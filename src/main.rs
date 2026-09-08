use regex_lite::Regex;
use std::collections::BTreeMap;
use zellij_tile::prelude::*;

#[cfg(test)]
mod tests;

/// The calls this plugin makes back into Zellij.
///
/// The event logic only talks to Zellij through this trait so it can be
/// exercised in native unit tests with a recording implementation (see
/// `tests.rs`); `ZellijHost` is the real implementation used at runtime.
trait Host {
    fn list_clients(&mut self);
    fn set_timeout(&mut self, seconds: f64);
    fn switch_to_input_mode(&mut self, mode: InputMode);
    fn hide_self(&mut self);
}

struct ZellijHost;

impl Host for ZellijHost {
    fn list_clients(&mut self) {
        zellij_tile::shim::list_clients();
    }

    fn set_timeout(&mut self, seconds: f64) {
        zellij_tile::shim::set_timeout(seconds);
    }

    fn switch_to_input_mode(&mut self, mode: InputMode) {
        zellij_tile::shim::switch_to_input_mode(&mode);
    }

    fn hide_self(&mut self) {
        zellij_tile::shim::hide_self();
    }
}

/// What the plugin last saw of the focused tab and pane. Every field is
/// unknown (`None` / empty) until the corresponding update arrives.
#[derive(Default)]
struct TabPane {
    /// Stable identity of the focused tab, used to detect tab changes. Tab
    /// positions shift when tabs are closed or moved, so a position alone
    /// cannot tell "the same tab" from "a different tab now at this position".
    tab_id: Option<usize>,
    /// Position of the focused tab, needed to look it up in a `PaneManifest`.
    tab_pos: Option<usize>,
    pane_id: Option<u32>,
    /// The focused pane's last seen command; `Some("")` means no command is
    /// running. Kept apart from `None` so that landing on an idle pane after a
    /// tab change still counts as a change and gets assessed.
    command: Option<String>,
}

struct State {
    permissions_granted: bool,
    is_enabled: bool,
    lock_regex: Option<Regex>,
    lock_triggers_deprecated: Option<Regex>,
    ignore_regex: Option<Regex>,
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
            lock_regex: Regex::new("^(vim|nvim)").ok(),
            lock_triggers_deprecated: None,
            ignore_regex: Regex::new("^(zellij|atuin history start.*)$").ok(),
            reaction_seconds: 0.3,
            timer_scheduled: false,
            current_mode: InputMode::Normal,
            latest_tab_pane: TabPane::default(),
            print_to_log: false,
        }
    }
}

// The macro exports `load`/`update`/`pipe`/`render` symbols and a `main` for
// the wasm module. Tests run natively with their own harness, so keep it out
// of test builds.
#[cfg(not(test))]
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
        // Zellij calls `load` exactly once per plugin instance, right after
        // instantiation on a fresh `State` (zellij-server plugin_loader.rs,
        // `load_plugin_instance`). A permission grant does not re-run it; the
        // `PermissionRequestResult` event is what hides the pane.
        self.load_configuration(configuration);
    }

    fn update(&mut self, event: Event) -> bool {
        self.handle_event(event, &mut ZellijHost)
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        self.handle_pipe(pipe_message, &mut ZellijHost)
    }

    fn render(&mut self, _rows: usize, _cols: usize) {}
}

fn parse_bool_config(value: &str) -> bool {
    matches!(value.trim(), "true" | "t" | "y" | "1")
}

/// The command a client's focused pane is running, or `""` if there is none.
///
/// Zellij reports `"N/A"` for a pane with no running command (an idle shell
/// prompt, a plugin pane, ...). This is the only place that maps it to empty.
fn running_command(client: &ClientInfo) -> &str {
    match client.running_command.trim() {
        "N/A" => "",
        cmd => cmd,
    }
}

/// True if `re` is set and matches any non-empty text in `texts`.
///
/// An unset rule never matches, and neither does empty text, so a catch-all
/// pattern like `.*` still leaves an idle pane (no running command) unlocked.
fn is_match(re: Option<&Regex>, texts: [&str; 2]) -> bool {
    re.is_some_and(|re| {
        texts
            .iter()
            .any(|text| !text.is_empty() && re.is_match(text))
    })
}

impl State {
    fn load_configuration(&mut self, configuration: BTreeMap<String, String>) {
        // Parsed first so that regex compile errors below can be logged.
        if let Some(print_to_log) = configuration.get("print_to_log") {
            self.print_to_log = parse_bool_config(print_to_log);
        }

        if let Some(is_enabled) = configuration.get("is_enabled") {
            self.is_enabled = parse_bool_config(is_enabled);
        }

        if let Some(lock_regex) = configuration.get("lock_regex") {
            self.lock_regex = self.compile_regex("lock_regex", lock_regex);
        }

        // TODO: delete deprecated `triggers` after v0.3.0 release
        if let Some(triggers) = configuration.get("triggers") {
            self.lock_triggers_deprecated =
                self.compile_regex("triggers", &format!("^({triggers})$"));
        }

        if let Some(ignore_regex) = configuration.get("ignore_regex") {
            self.ignore_regex = self.compile_regex("ignore_regex", ignore_regex);
        }

        if let Some(reaction_seconds) = configuration.get("reaction_seconds") {
            self.reaction_seconds = reaction_seconds.parse::<f64>().unwrap();
        }

        if self.print_to_log {
            eprintln!("[autolock] Configuration loaded.");
            eprintln!("[autolock] Enabled: {}", self.is_enabled);
            eprintln!(
                "[autolock] Lock Commands: {:?}",
                self.lock_regex.as_ref().map(Regex::as_str)
            );
            eprintln!(
                "[autolock] Ignore Commands: {:?}",
                self.ignore_regex.as_ref().map(Regex::as_str)
            );
            eprintln!("[autolock] Reaction seconds: {}", self.reaction_seconds);
        }
    }

    /// Compile a configured pattern once. An empty pattern disables the rule.
    /// An invalid pattern also disables it (fails closed) and is logged once,
    /// here, rather than on every check.
    fn compile_regex(&self, name: &str, pattern: &str) -> Option<Regex> {
        if pattern.is_empty() {
            return None;
        }
        match Regex::new(pattern) {
            Ok(re) => Some(re),
            Err(e) => {
                if self.print_to_log {
                    eprintln!("[autolock] Invalid `{name}` pattern {pattern:?}: {e}");
                }
                None
            }
        }
    }

    fn handle_event(&mut self, event: Event, host: &mut impl Host) -> bool {
        match event {
            Event::PermissionRequestResult(permission) => {
                self.permissions_granted = matches!(permission, PermissionStatus::Granted);
                if self.permissions_granted {
                    host.hide_self();
                }
            }

            Event::InputReceived => {
                self.start_timer(host);
            }

            Event::ModeUpdate(mode_info) => {
                self.current_mode = mode_info.mode;
                self.start_timer(host);
            }

            Event::TabUpdate(tab_info) => {
                if let Some(tab) = get_focused_tab(&tab_info)
                    && Some(tab.tab_id) != self.latest_tab_pane.tab_id
                {
                    self.latest_tab_pane = TabPane {
                        tab_id: Some(tab.tab_id),
                        tab_pos: Some(tab.position),
                        ..Default::default()
                    };
                    // Zellij sends `PaneUpdate` *before* `TabUpdate` on a tab switch, so the
                    // pane handler above has already seen (and ignored) the new tab's pane
                    // against the old tab position. Ask for the command now rather than
                    // waiting for the debounce timer.
                    host.list_clients();
                }
            }

            Event::PaneUpdate(pane_manifest) => {
                // Until a `TabUpdate` has told us which tab is focused there is
                // nothing to look the pane up in.
                if let Some(tab_pos) = self.latest_tab_pane.tab_pos
                    && let Some(pane) = get_focused_pane(tab_pos, &pane_manifest)
                    && Some(pane.id) != self.latest_tab_pane.pane_id
                {
                    self.latest_tab_pane.pane_id = Some(pane.id);
                    host.list_clients();
                }
            }

            Event::ListClients(clients) => {
                if !self.is_enabled {
                    return false;
                }

                if let Some(current_client) = clients.iter().find(|client| client.is_current_client)
                {
                    let running_command = running_command(current_client);

                    let command_changed =
                        self.latest_tab_pane.command.as_deref() != Some(running_command);

                    if command_changed {
                        self.latest_tab_pane.command = Some(running_command.to_string());

                        let target_input_mode = self.determine_target_mode(running_command);

                        // Only switch if the mode is actually changing, and
                        // if the current input mode is `Locked` or `Normal`
                        if self.current_mode != target_input_mode
                            && (self.current_mode == InputMode::Locked
                                || self.current_mode == InputMode::Normal)
                        {
                            host.switch_to_input_mode(target_input_mode);
                        }

                        // If the command changed, perform another iteration.
                        self.start_timer(host);
                    }
                }
            }

            Event::Timer(_t) => {
                host.list_clients();
                self.timer_scheduled = false;
            }

            _ => {}
        }
        false // No need to render UI.
    }

    fn handle_pipe(&mut self, pipe_message: PipeMessage, host: &mut impl Host) -> bool {
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
                other => {
                    if self.print_to_log {
                        eprintln!(
                            "[autolock] Unknown pipe payload {other:?}; expected `enable`, `disable`, or `toggle`."
                        );
                    }
                }
            }
        }

        if self.is_enabled {
            host.list_clients();
            self.start_timer(host);
        }

        false // No need to render UI.
    }

    fn start_timer(&mut self, host: &mut impl Host) {
        if self.is_enabled && !self.timer_scheduled {
            host.set_timeout(self.reaction_seconds);
            self.timer_scheduled = true;
        }
    }

    fn determine_target_mode(&self, running_command: &str) -> InputMode {
        let running_command_exe = running_command
            .split_whitespace()
            .next()
            .and_then(|cmd| cmd.split('/').next_back())
            .unwrap_or("")
            .trim_matches(['(', ')']);

        let texts = [running_command, running_command_exe];
        let lock = is_match(self.lock_regex.as_ref(), texts)
            || is_match(self.lock_triggers_deprecated.as_ref(), texts);
        let ignore = is_match(self.ignore_regex.as_ref(), texts);

        let engage = lock && !ignore;

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
}
