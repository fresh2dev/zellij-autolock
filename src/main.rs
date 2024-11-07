use std::collections::BTreeMap;
use zellij_tile::prelude::*;
use zellij_tile::shim::list_clients;

struct TabPane {
    tab_pos: usize,
    pane_id: u32,
}

struct State {
    permissions_granted: bool,
    lock_trigger_cmds: Vec<String>,
    reaction_seconds: f64,
    timer_scheduled: bool,
    latest_tab_pane: TabPane,
    latest_mode: InputMode,
    latest_running_command: String,
    print_to_log: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            permissions_granted: false,
            lock_trigger_cmds: vec!["vim".to_string(), "nvim".to_string()],
            reaction_seconds: 0.3,
            timer_scheduled: false,
            latest_tab_pane: TabPane {
                tab_pos: usize::MAX,
                pane_id: u32::MAX,
            },
            latest_mode: InputMode::Normal,
            latest_running_command: "".to_string(),
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

            Event::ModeUpdate(mode_info) => {
                self.latest_mode = mode_info.mode;
                self.start_timer();
            }

            Event::InputReceived => {
                self.start_timer();
            }

            Event::TabUpdate(tab_info) => {
                if let Some(tab) = get_focused_tab(&tab_info) {
                    if tab.position != self.latest_tab_pane.tab_pos {
                        self.latest_tab_pane = TabPane {
                            tab_pos: tab.position,
                            pane_id: u32::MAX,
                        };
                    }
                }
            }

            Event::PaneUpdate(pane_manifest) => {
                let focused_pane =
                    get_focused_pane(self.latest_tab_pane.tab_pos, &pane_manifest).clone();

                if let Some(pane) = focused_pane {
                    if pane.id != self.latest_tab_pane.pane_id {
                        self.latest_tab_pane = TabPane {
                            tab_pos: self.latest_tab_pane.tab_pos,
                            pane_id: pane.id,
                        };

                        list_clients();
                    }
                }
            }

            Event::ListClients(clients) => {
                if let Some(current_client) = clients
                    .iter()
                    .find(|client| client.is_current_client && !client.running_command.is_empty())
                {
                    let running_command = current_client.running_command.trim().to_string();

                    if self.print_to_log {
                        eprintln!("Detected command in pane: {}!", running_command);
                    }

                    let mut is_trigger_cmd = false;

                    if running_command != "N/A" {
                        let running_command_base =
                            running_command.split_whitespace().collect::<Vec<_>>()[0].to_string();

                        is_trigger_cmd = self.lock_trigger_cmds.contains(&running_command)
                            || self.lock_trigger_cmds.contains(&running_command_base);
                    }

                    let target_input_mode = if is_trigger_cmd {
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

                    if running_command != self.latest_running_command {
                        self.latest_running_command = running_command;
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
        return false; // No need to render UI.
    }

    fn pipe(&mut self, _pipe_message: PipeMessage) -> bool {
        list_clients();
        self.start_timer();
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
        if let Some(reaction_seconds) = configuration.get("reaction_seconds") {
            self.reaction_seconds = reaction_seconds.parse::<f64>().unwrap();
        }
        if let Some(print_to_log) = configuration.get("print_to_log") {
            self.print_to_log = matches!(print_to_log.trim(), "true" | "t" | "y" | "1");
        }
    }
    fn start_timer(&mut self) {
        if !self.timer_scheduled {
            set_timeout(self.reaction_seconds);
            self.timer_scheduled = true;
        }
    }
}
