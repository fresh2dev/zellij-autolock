use std::collections::BTreeMap;
use zellij_tile::prelude::*;

/// Extracts the current running command from
/// the CLI command `zellij action list-clients`.
/// src: https://github.com/hiasr/vim-zellij-navigator/blob/main/src/main.rs
fn term_command_from_client_list(cl: String) -> Option<String> {
    let clients = cl.split('\n').skip(1).collect::<Vec<&str>>();
    if clients.is_empty() {
        return None;
    }

    let columns = clients[0].split_whitespace().collect::<Vec<&str>>();
    if columns.len() < 3 {
        return None;
    }

    let is_terminal = columns[1].starts_with("terminal");
    let no_command = columns[2] == "N/A";
    if !is_terminal || no_command {
        return None;
    }

    let command = columns[2].split('/').last()?;
    return Some(command.to_string());
}

fn list_clients() {
    run_command(
        &["zellij", "action", "list-clients"],
        BTreeMap::from([("action".to_owned(), "list-clients".to_owned())]),
    );
}

struct TabPane {
    tab_pos: usize,
    pane_id: u32,
    term_cmd: Option<String>,
}

struct State {
    permissions_granted: bool,
    lock_trigger_cmds: Vec<String>,
    lock_watch_cmds: Vec<String>,
    watch_interval: f64,
    timer_active: bool,
    timer_scheduled: bool,
    latest_tab_pane: TabPane,
    latest_mode: InputMode,
}

impl Default for State {
    fn default() -> Self {
        Self {
            permissions_granted: false,
            lock_trigger_cmds: vec!["vim".to_string(), "nvim".to_string()],
            lock_watch_cmds: vec![],
            watch_interval: 1.0,
            timer_active: false,
            timer_scheduled: false,
            latest_tab_pane: TabPane {
                tab_pos: usize::MAX,
                pane_id: u32::MAX,
                term_cmd: None,
            },
            latest_mode: InputMode::Normal,
        }
    }
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        request_permission(&[
            PermissionType::RunCommands,
            PermissionType::ChangeApplicationState,
            PermissionType::ReadApplicationState,
        ]);
        subscribe(&[
            EventType::PermissionRequestResult,
            EventType::ModeUpdate,
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::RunCommandResult,
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

            Event::ModeUpdate(mode_info) => {
                self.latest_mode = mode_info.mode;
            }

            Event::TabUpdate(tab_info) => {
                if let Some(tab) = get_focused_tab(&tab_info) {
                    if tab.position != self.latest_tab_pane.tab_pos {
                        // eprintln!("Tab focus changed!");
                        self.latest_tab_pane = TabPane {
                            tab_pos: tab.position,
                            pane_id: u32::MAX,
                            term_cmd: None,
                        };

                        list_clients();
                    }
                }
            }

            Event::PaneUpdate(pane_manifest) => {
                let focused_pane =
                    get_focused_pane(self.latest_tab_pane.tab_pos, &pane_manifest).clone();

                if let Some(pane) = focused_pane {
                    if pane.id != self.latest_tab_pane.pane_id
                        || pane.terminal_command != self.latest_tab_pane.term_cmd
                    {
                        // eprintln!("Pane focus changed!");
                        self.latest_tab_pane = TabPane {
                            tab_pos: self.latest_tab_pane.tab_pos,
                            pane_id: pane.id,
                            term_cmd: pane.terminal_command,
                        };

                        list_clients();
                    }
                }
            }

            Event::RunCommandResult(_exit_code, stdout, _stderr, context) => {
                if context.get("action") == Some(&"list-clients".to_owned()) {
                    let stdout = String::from_utf8(stdout).unwrap();
                    if stdout.len() == 0 {
                        // The `list-clients` command sometimes bugs out and returns nothing.
                        // When this happens, activate the timer to repeat another cycle.
                        self.timer_active = true;
                    } else {
                        // eprintln!("list-clients output: {}", stdout);

                        let pane_cmd = if let Some(cmd) = term_command_from_client_list(stdout) {
                            cmd.clone()
                        // } else if let Some(cmd) = &self.latest_tab_pane.term_cmd {
                        //     cmd.split_whitespace().next().unwrap_or("").to_string()
                        } else {
                            "".to_string()
                        };

                        eprintln!("Detected command in pane: {}", pane_cmd);

                        let is_watched_cmd = self.lock_watch_cmds.contains(&pane_cmd);

                        let target_input_mode =
                            if is_watched_cmd || self.lock_trigger_cmds.contains(&pane_cmd) {
                                InputMode::Locked
                            } else if self.latest_mode == InputMode::Locked {
                                InputMode::Normal
                            } else {
                                self.latest_mode
                            };

                        if self.latest_mode != target_input_mode
                            && (self.latest_mode == InputMode::Locked
                                || self.latest_mode == InputMode::Normal)
                        {
                            switch_to_input_mode(&target_input_mode);
                        }

                        self.timer_active = is_watched_cmd;
                    }

                    if self.timer_active && !self.timer_scheduled {
                        set_timeout(self.watch_interval);
                        self.timer_scheduled = true;
                    }
                }
            }

            Event::Timer(_t) => {
                list_clients();
                self.timer_scheduled = false;
            }

            _ => {}
        }
        return false; // No need to render UI.
    }

    fn pipe(&mut self, _pipe_message: PipeMessage) -> bool {
        list_clients();
        return false; // No need to render UI.
    }

    fn render(&mut self, _rows: usize, _cols: usize) {}
}

impl State {
    fn load_configuration(&mut self, configuration: BTreeMap<String, String>) {
        if let Some(lock_trigger_cmds) = configuration.get("triggers") {
            self.lock_trigger_cmds = lock_trigger_cmds
                .split('|')
                .map(|s| s.trim().to_string())
                .collect();
        }
        if let Some(lock_watch_cmds) = configuration.get("watch_triggers") {
            self.lock_watch_cmds = lock_watch_cmds
                .split('|')
                .map(|s| s.trim().to_string())
                .collect();
        }
        if let Some(watch_interval) = configuration.get("watch_interval") {
            self.watch_interval = watch_interval.parse::<f64>().unwrap();
        }
    }
}
