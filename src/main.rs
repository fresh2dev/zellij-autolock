use regex::Regex;
use std::collections::BTreeMap;
use zellij_tile::prelude::*;
use zellij_tile::shim::list_clients;

struct TabPane {
    tab_pos: usize,
    pane_id: u32,
    command: String,
}

struct State {
    permissions_granted: bool,
    is_enabled: bool,
    lock_regex: String,
    lock_triggers_deprecated: String,
    ignore_regex: String,
    reaction_seconds: f64,
    timer_scheduled: bool,
    current_mode: InputMode,
    latest_tab_pane: TabPane,
    print_to_log: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            permissions_granted: false,
            is_enabled: true,
            lock_regex: "^(vim|nvim)".to_string(),
            lock_triggers_deprecated: "".to_string(),
            ignore_regex: "^(zellij|atuin history start.*)$".to_string(),
            reaction_seconds: 0.3,
            timer_scheduled: false,
            current_mode: InputMode::Normal,
            latest_tab_pane: TabPane {
                tab_pos: usize::MAX,
                pane_id: u32::MAX,
                command: "".to_string(),
            },
            print_to_log: false,
        }
    }
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        request_permission(&[
            // PermissionType::RunCommands,
            PermissionType::ChangeApplicationState,
            PermissionType::ReadApplicationState,
        ]);
        subscribe(&[
            EventType::InputReceived,
            EventType::ListClients,
            EventType::ModeUpdate,
            EventType::PaneUpdate,
            EventType::PermissionRequestResult,
            EventType::TabUpdate,
            EventType::Timer,
        ]);
        if self.permissions_granted {
            hide_self();
        }
        self.load_configuration(configuration);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(permission) => {
                self.permissions_granted = match permission {
                    PermissionStatus::Granted => true,
                    PermissionStatus::Denied => false,
                };
                if self.permissions_granted {
                    hide_self();
                }
            }

            Event::InputReceived => {
                self.start_timer();
            }

            Event::ModeUpdate(mode_info) => {
                self.current_mode = mode_info.mode;
                self.start_timer();
            }

            Event::TabUpdate(tab_info) => {
                if let Some(tab) = get_focused_tab(&tab_info)
                    && tab.position != self.latest_tab_pane.tab_pos
                {
                    self.latest_tab_pane = TabPane {
                        tab_pos: tab.position,
                        pane_id: u32::MAX,
                        command: "".to_string(),
                    };
                }
            }

            Event::PaneUpdate(pane_manifest) => {
                let focused_pane =
                    get_focused_pane(self.latest_tab_pane.tab_pos, &pane_manifest).clone();

                if let Some(pane) = focused_pane
                    && pane.id != self.latest_tab_pane.pane_id
                {
                    self.latest_tab_pane.pane_id = pane.id;
                    list_clients();
                }
            }

            Event::ListClients(clients) => {
                if !self.is_enabled {
                    return false;
                }

                if let Some(current_client) = clients.iter().find(|client| client.is_current_client)
                {
                    let running_command = match current_client.running_command.trim() {
                        "N/A" => "",
                        cmd => cmd,
                    };

                    let command_changed = running_command != self.latest_tab_pane.command;

                    if command_changed {
                        self.latest_tab_pane.command = running_command.to_string();

                        let target_input_mode = self.determine_target_mode(running_command);

                        // Only switch if the mode is actually changing, and
                        // if the current input mode is `Locked` or `Normal`
                        if self.current_mode != target_input_mode
                            && (self.current_mode == InputMode::Locked
                                || self.current_mode == InputMode::Normal)
                        {
                            switch_to_input_mode(&target_input_mode);
                        }

                        // If the command changed, perform another iteration.
                        self.start_timer();
                    }
                }
            }

            Event::Timer(_t) => {
                list_clients();
                self.timer_scheduled = false;
            }

            _ => {}
        }
        false // No need to render UI.
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        if let Some(payload) = pipe_message.payload {
            let action = payload.to_string();
            match action.as_str() {
                "enable" => {
                    self.is_enabled = true;
                    if self.print_to_log {
                        eprintln!("[autolock] Enabled");
                    }
                }
                "disable" => {
                    self.is_enabled = false;
                    if self.print_to_log {
                        eprintln!("[autolock] Disabled");
                    }
                }
                "toggle" => {
                    self.is_enabled = !self.is_enabled;
                    if self.print_to_log {
                        eprintln!("[autolock] Enabled: {}", self.is_enabled);
                    }
                }
                _ => {}
            }
        }

        if self.is_enabled {
            list_clients();
            self.start_timer();
        }

        false // No need to render UI.
    }

    fn render(&mut self, _rows: usize, _cols: usize) {}
}

fn parse_bool_config(value: &str) -> bool {
    matches!(value.trim(), "true" | "t" | "y" | "1")
}

impl State {
    fn load_configuration(&mut self, configuration: BTreeMap<String, String>) {
        if let Some(is_enabled) = configuration.get("is_enabled") {
            self.is_enabled = parse_bool_config(is_enabled);
        }

        if let Some(lock_regex) = configuration.get("lock_regex") {
            self.lock_regex = lock_regex.to_string();
        }

        // TODO: delete deprecated `triggers` after v0.3.0 release
        if let Some(lock_triggers_deprecated) = configuration.get("triggers") {
            self.lock_triggers_deprecated = format!("^({})$", lock_triggers_deprecated);
        }

        if let Some(ignore_regex) = configuration.get("ignore_regex") {
            self.ignore_regex = ignore_regex.to_string();
        }

        if let Some(reaction_seconds) = configuration.get("reaction_seconds") {
            self.reaction_seconds = reaction_seconds.parse::<f64>().unwrap();
        }

        if let Some(print_to_log) = configuration.get("print_to_log") {
            self.print_to_log = parse_bool_config(print_to_log);
        }

        if self.print_to_log {
            eprintln!("[autolock] Configuration loaded.");
            eprintln!("[autolock] Enabled: {}", self.is_enabled);
            eprintln!("[autolock] Lock Commands: {:?}", self.lock_regex);
            eprintln!("[autolock] Ignore Commands: {:?}", self.ignore_regex);
            eprintln!("[autolock] Reaction seconds: {}", self.reaction_seconds);
        }
    }

    fn start_timer(&mut self) {
        if self.is_enabled && !self.timer_scheduled {
            set_timeout(self.reaction_seconds);
            self.timer_scheduled = true;
        }
    }

    fn determine_target_mode(&self, running_command: &str) -> InputMode {
        let running_command_exe = match running_command
            .split_whitespace()
            .next()
            .and_then(|cmd| cmd.split('/').next_back())
            .unwrap_or("")
            .trim_matches(['(', ')'])
        {
            "N/A" => "",
            cmd_exe => cmd_exe,
        };

        let lock = self.is_regex_match(&self.lock_regex, running_command_exe, running_command);

        let lock_deprecated = self.is_regex_match(
            &self.lock_triggers_deprecated,
            running_command,
            running_command_exe,
        );

        let ignore = self.is_regex_match(&self.ignore_regex, running_command, running_command_exe);

        let engage = (lock || lock_deprecated) && !ignore;

        if self.print_to_log {
            eprintln!(
                "[autolock] Detected command: `{}`; Executable: `{}`; Is trigger? {}.",
                running_command, running_command_exe, engage,
            );
        }

        if engage {
            InputMode::Locked
        } else {
            InputMode::Normal
        }
    }

    fn _is_regex_match(&self, pattern: &str, text: &str) -> bool {
        if pattern.is_empty() || text.is_empty() {
            return false;
        }
        match Regex::new(pattern) {
            Ok(re) => re.is_match(text),
            Err(e) => {
                if self.print_to_log {
                    eprintln!(
                        "[autolock] Invalid regex pattern: '{}'. Error: {}",
                        pattern, e
                    );
                }
                false
            }
        }
    }
    fn is_regex_match(&self, pattern: &str, command: &str, command_exe: &str) -> bool {
        self._is_regex_match(pattern, command) || self._is_regex_match(pattern, command_exe)
    }
}
