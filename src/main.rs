use std::collections::BTreeMap;
use zellij_tile::prelude::*;

struct TabPane {
    tab_pos: usize,
    pane_id: PaneId,
}

struct State {
    is_enabled: bool,
    permissions_granted: bool,
    lock_trigger_cmds: Vec<String>,
    latest_tab_pane: TabPane,
    latest_mode: InputMode,
    print_to_log: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            is_enabled: true,
            permissions_granted: false,
            lock_trigger_cmds: vec!["vim".to_string(), "nvim".to_string()],
            latest_tab_pane: TabPane {
                tab_pos: usize::MAX,
                pane_id: PaneId::Terminal(u32::MAX),
            },
            latest_mode: InputMode::Normal,
            print_to_log: false,
        }
    }
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        request_permission(&[
            PermissionType::ChangeApplicationState,
            PermissionType::ReadApplicationState,
        ]);
        subscribe(&[
            EventType::CommandChanged,
            EventType::ModeUpdate,
            EventType::PaneUpdate,
            EventType::PermissionRequestResult,
            EventType::TabUpdate,
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
                        self.latest_tab_pane = TabPane {
                            tab_pos: tab.position,
                            pane_id: PaneId::Terminal(u32::MAX),
                        };
                    }
                }
            }

            Event::PaneUpdate(pane_manifest) => {
                let focused_pane =
                    get_focused_pane(self.latest_tab_pane.tab_pos, &pane_manifest).clone();

                if let Some(pane) = focused_pane {
                    let pane_id = PaneId::Terminal(pane.id);
                    if pane_id != self.latest_tab_pane.pane_id {
                        self.latest_tab_pane.pane_id = pane_id;

                        if self.is_enabled {
                            if let Ok(cmd) = get_pane_running_command(pane_id) {
                                if self.print_to_log {
                                    eprintln!(
                                        "[autolock] Pane switch detected. Command: {:?}",
                                        cmd
                                    );
                                }
                                self.check_and_switch_mode_for_command(&cmd);
                            }
                        }
                    }
                }
            }

            Event::CommandChanged(_pane_id, command, is_foreground, _focused_client_ids) => {
                if self.is_enabled && is_foreground {
                    if self.print_to_log {
                        eprintln!(
                            "[autolock] CommandChanged: {:?}, is_foreground: {}",
                            command, is_foreground
                        );
                    }
                    self.check_and_switch_mode_for_command(&command);
                }
            }

            _ => {}
        }
        return false;
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        if let Some(payload) = pipe_message.payload {
            let action = payload.to_string();

            if action == "enable" {
                self.is_enabled = true;
                if self.print_to_log {
                    eprintln!("[autolock] Enabled");
                }
            } else if action == "disable" {
                self.is_enabled = false;
                if self.print_to_log {
                    eprintln!("[autolock] Disabled");
                }
            } else if action == "toggle" {
                self.is_enabled = !self.is_enabled;
                if self.print_to_log {
                    eprintln!("[autolock] Enabled: {}", self.is_enabled);
                }
            }
        }

        if self.is_enabled {
            if let Ok((_tab_index, pane_id)) = get_focused_pane_info() {
                if let Ok(cmd) = get_pane_running_command(pane_id) {
                    if self.print_to_log {
                        eprintln!("[autolock] Pipe triggered check. Command: {:?}", cmd);
                    }
                    self.check_and_switch_mode_for_command(&cmd);
                }
            }
        }

        return false;
    }

    fn render(&mut self, _rows: usize, _cols: usize) {}
}

impl State {
    fn check_and_switch_mode_for_command(&self, command: &[String]) {
        if command.is_empty() {
            return;
        }
        let executable = &command[0];
        let basename = executable.rsplit('/').next().unwrap_or(executable);

        let is_trigger = self.lock_trigger_cmds.iter().any(|t| t == executable)
            || self.lock_trigger_cmds.iter().any(|t| t == basename);

        let target_input_mode = if is_trigger {
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
    }

    fn load_configuration(&mut self, configuration: BTreeMap<String, String>) {
        if let Some(is_enabled) = configuration.get("is_enabled") {
            self.is_enabled = matches!(is_enabled.trim(), "true" | "t" | "y" | "1");
        }
        if let Some(lock_trigger_cmds) = configuration.get("triggers") {
            self.lock_trigger_cmds = lock_trigger_cmds
                .split('|')
                .map(|s| s.trim().to_string())
                .collect();
        }
        if let Some(print_to_log) = configuration.get("print_to_log") {
            self.print_to_log = matches!(print_to_log.trim(), "true" | "t" | "y" | "1");
        }

        if self.print_to_log {
            eprintln!("[autolock] Configuration loaded.");
            eprintln!("[autolock] Enabled: {}", self.is_enabled);
            eprintln!("[autolock] Trigger commands: {:?}", self.lock_trigger_cmds);
        }
    }
}
