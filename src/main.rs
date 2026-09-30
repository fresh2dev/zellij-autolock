use regex_lite::Regex;
use std::collections::BTreeMap;
use zellij_tile::prelude::*;

#[cfg(test)]
mod tests;

/// How long to wait before looking at the focused pane again, after input or
/// after its command changed. Shorter risks looking before a program has
/// started or exited; longer slows switching down.
const RECHECK_DELAY_SECONDS: f64 = 0.3;

/// How many times to look again, `RECHECK_DELAY_SECONDS` apart, after the
/// focused pane's command changed: about 1.5 s, one period of Zellij's
/// `CommandChanged` ticker plus margin. A command that exits before the ticker
/// sees it produces no event for the exit, so only these checks notice it.
const RECHECK_ROUNDS: u32 = 5;

/// Lock for every command, except those `DEFAULT_IGNORE_REGEX` matches.
const DEFAULT_LOCK_REGEX: &str = ".*";

/// Common shells and prompt tooling. A pane idle at its prompt reports the
/// shell itself, and a background plugin cannot learn Zellij's default shell,
/// so shells must be listed here or idle panes lock. The executable keeps its
/// extension on Windows, hence the optional `.exe`.
const DEFAULT_IGNORE_REGEX: &str = r"^(zellij|sh|ash|dash|bash|zsh|fish|ksh|mksh|csh|tcsh|nu|xonsh|elvish|pwsh|powershell|cmd|ls|lsd|eza|starship|direnv|\(atuin\)|atuin history start.*)(\.exe)?$";

/// The calls this plugin makes back into Zellij.
///
/// The event logic only talks to Zellij through this trait so it can be
/// exercised in native unit tests with a recording implementation (see
/// `tests.rs`); `ZellijHost` is the real implementation used at runtime.
///
/// The query methods (`focused_pane`, `pane_command`, `plugin_url`) are
/// synchronous and need `ReadApplicationState`. Only call them once that
/// permission is granted: Zellij sends no reply to a denied query, and the
/// shim panics waiting for one.
trait Host {
    /// The pane this plugin instance's client has focused, in any layer,
    /// terminal or plugin.
    fn focused_pane(&mut self) -> Result<PaneId, String>;

    /// The argv running in a terminal pane: its foreground process if one is
    /// running, else the pane's own process (e.g. the idle shell). Errors for
    /// plugin panes, unknown panes, and query timeouts.
    fn pane_command(&mut self, pane: PaneId) -> Result<Vec<String>, String>;

    /// Where a plugin pane was loaded from, e.g. `zellij:session-manager`.
    fn plugin_url(&mut self, pane: PaneId) -> Option<String>;

    fn set_timeout(&mut self, seconds: f64);
    fn switch_to_input_mode(&mut self, mode: InputMode);
    fn hide_self(&mut self);
}

struct ZellijHost;

impl Host for ZellijHost {
    fn focused_pane(&mut self) -> Result<PaneId, String> {
        // The first element is the focused tab's id (not its position,
        // despite the shim's doc comment). The pane id alone is enough.
        zellij_tile::shim::get_focused_pane_info().map(|(_tab_id, pane)| pane)
    }

    fn pane_command(&mut self, pane: PaneId) -> Result<Vec<String>, String> {
        zellij_tile::shim::get_pane_running_command(pane)
    }

    fn plugin_url(&mut self, pane: PaneId) -> Option<String> {
        zellij_tile::shim::get_pane_info(pane)?.plugin_url
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

/// What the plugin last saw of its client's focused pane. Both fields stay
/// unknown (`None`) until a query for them succeeds.
#[derive(Default)]
struct Focus {
    pane: Option<PaneId>,
    /// The argv last assessed for `pane`. A focus change resets it to `None`,
    /// so a pane that runs the same command as the previous one still gets
    /// assessed afresh.
    command: Option<Vec<String>>,
}

/// Severity of a log line, lowest first. Lines below `State::log_level`
/// are not written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Critical,
}

impl LogLevel {
    /// Parse a configured level name, case-insensitively.
    fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "trace" => Some(Self::Trace),
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" => Some(Self::Warn),
            "error" => Some(Self::Error),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }

    /// The tag written in front of each log line.
    fn tag(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Critical => "CRITICAL",
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.tag())
    }
}

struct State {
    permissions_granted: bool,
    is_enabled: bool,
    lock_regex: Option<Regex>,
    lock_triggers_deprecated: Option<Regex>,
    ignore_regex: Option<Regex>,
    /// Timers set and not yet fired. `set_timeout` cannot be cancelled, so
    /// only the last timer set runs a check; the ones it superseded are
    /// ignored when they fire.
    timers_pending: u32,
    /// Checks still due after the last command change (see `RECHECK_ROUNDS`).
    rechecks_left: u32,
    current_mode: InputMode,
    focus: Focus,
    log_level: LogLevel,
    /// The configuration block last applied, to recognise the unchanged
    /// blocks Zellij sends again on unrelated config reloads.
    configuration: BTreeMap<String, String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            permissions_granted: false,
            is_enabled: true,
            lock_regex: Regex::new(DEFAULT_LOCK_REGEX).ok(),
            lock_triggers_deprecated: None,
            ignore_regex: Regex::new(DEFAULT_IGNORE_REGEX).ok(),
            timers_pending: 0,
            rechecks_left: 0,
            current_mode: InputMode::Normal,
            focus: Focus::default(),
            log_level: LogLevel::Info,
            configuration: BTreeMap::new(),
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
            EventType::CommandChanged,
            // Subscribed only so that Zellij sends `ModeUpdate` without the
            // full keybinding table, which this plugin never reads. The
            // `InitialKeybinds` event itself is ignored.
            EventType::InitialKeybinds,
            EventType::InputReceived,
            EventType::ModeUpdate,
            EventType::PaneUpdate,
            EventType::PermissionRequestResult,
            EventType::PluginConfigurationChanged,
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

/// True if `re` is set and matches any non-empty text in `texts`.
///
/// An unset rule never matches, and neither does empty text, so a catch-all
/// pattern like `.*` never locks on a pane that reports no argv at all.
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
        // `print_to_log` is deprecated (an alias for `debug`), slated for
        // removal after 0.3.0; `log_level` wins when both are set.
        if configuration
            .get("print_to_log")
            .is_some_and(|value| parse_bool_config(value))
        {
            self.log_level = LogLevel::Debug;
        }
        let unrecognised_log_level = match configuration.get("log_level") {
            Some(value) => match LogLevel::parse(value) {
                Some(level) => {
                    self.log_level = level;
                    None
                }
                None => Some(value),
            },
            None => None,
        };

        if let Some(is_enabled) = configuration.get("is_enabled") {
            self.is_enabled = parse_bool_config(is_enabled);
        }

        // The default rules only make sense together (lock everything, except
        // shells). A config that sets any rule starts from none, so e.g. a 0.2
        // `triggers` allowlist or a custom `lock_regex` does not inherit parts
        // of the defaults.
        if ["lock_regex", "ignore_regex", "triggers"]
            .iter()
            .any(|key| configuration.contains_key(*key))
        {
            self.lock_regex = None;
            self.ignore_regex = None;
            self.lock_triggers_deprecated = None;
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

        self.log(LogLevel::Info, format_args!("Configuration loaded."));
        if let Some(value) = unrecognised_log_level {
            self.log(
                LogLevel::Warn,
                format_args!(
                    "Unrecognised log_level {value:?}; using {}",
                    self.log_level.tag().to_ascii_lowercase()
                ),
            );
        }
        self.log(LogLevel::Info, format_args!("Enabled: {}", self.is_enabled));
        self.log(
            LogLevel::Debug,
            format_args!(
                "Lock Commands: {:?}",
                self.lock_regex.as_ref().map(Regex::as_str)
            ),
        );
        self.log(
            LogLevel::Debug,
            format_args!(
                "Ignore Commands: {:?}",
                self.ignore_regex.as_ref().map(Regex::as_str)
            ),
        );

        self.configuration = configuration;
    }

    /// Apply a configuration block that changed while the plugin runs.
    ///
    /// Zellij sends the plugin's whole block, and decides whether it changed
    /// by comparing it with the block the plugin was *loaded* with. Once it
    /// has changed, every later config reload sends it again, so a block
    /// identical to the one applied is ignored. Otherwise every setting is
    /// rebuilt from its default, so a key removed from the block reverts.
    fn apply_configuration_change(
        &mut self,
        configuration: BTreeMap<String, String>,
        host: &mut impl Host,
    ) {
        if configuration == self.configuration {
            return;
        }
        self.log(LogLevel::Info, format_args!("Configuration changed."));

        let was_enabled = self.is_enabled;
        // `is_enabled` is the starting state, and the pipes may have flipped
        // it since. Keep that unless this change edits the setting itself.
        let is_enabled_changed =
            configuration.get("is_enabled") != self.configuration.get("is_enabled");

        let State {
            is_enabled,
            lock_regex,
            lock_triggers_deprecated,
            ignore_regex,
            log_level,
            ..
        } = State::default();
        self.is_enabled = is_enabled;
        self.lock_regex = lock_regex;
        self.lock_triggers_deprecated = lock_triggers_deprecated;
        self.ignore_regex = ignore_regex;
        self.log_level = log_level;

        self.load_configuration(configuration);
        if !is_enabled_changed {
            self.is_enabled = was_enabled;
        }

        if self.is_enabled && !was_enabled {
            // Nothing was tracked while disabled, as when a pipe enables it.
            self.focus = Focus::default();
        } else {
            // The new rules may judge the same command differently.
            self.focus.command = None;
        }
        self.recheck(host);
    }

    /// Write one line to the Zellij log, tagged with its level. Lines below
    /// `log_level` are dropped.
    fn log(&self, level: LogLevel, args: std::fmt::Arguments) {
        if level >= self.log_level {
            eprintln!("[autolock] [{level}] {args}");
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
                self.log(
                    LogLevel::Error,
                    format_args!("Invalid `{name}` pattern {pattern:?}: {e}"),
                );
                None
            }
        }
    }

    fn handle_event(&mut self, event: Event, host: &mut impl Host) -> bool {
        self.log(
            LogLevel::Trace,
            format_args!("Event: {}", event_name(&event)),
        );
        match event {
            Event::PermissionRequestResult(permission) => {
                self.permissions_granted = matches!(permission, PermissionStatus::Granted);
                if self.permissions_granted {
                    host.hide_self();
                    // Zellij re-sends a remembered grant on every load, so this
                    // is where the plugin first looks at the focused pane.
                    self.refresh_focus(host);
                }
            }

            // Every input pushes the check back, so it runs a moment after
            // the last key rather than the first (e.g. after `Enter`, not
            // after the first letter of the command typed before it).
            Event::InputReceived => {
                self.restart_recheck(host);
            }

            // Other modes (Scroll, Pane, ...) are never overridden, so a pane
            // focused or a command changed during one was not switched for.
            // Back in Normal, look afresh. Going straight to Locked is a
            // choice of its own and is left alone.
            Event::ModeUpdate(mode_info) => {
                let from_other_mode =
                    !matches!(self.current_mode, InputMode::Normal | InputMode::Locked);
                self.current_mode = mode_info.mode;
                if from_other_mode && self.current_mode == InputMode::Normal {
                    self.focus.command = None;
                    self.recheck(host);
                }
            }

            // Neither payload says which pane *this* client has focused: the
            // manifest marks a pane focused if any client focuses it, and on a
            // tab switch `PaneUpdate` arrives before `TabUpdate`. Ask instead.
            Event::TabUpdate(_) | Event::PaneUpdate(_) => {
                self.refresh_focus(host);
            }

            Event::Timer(_t) => {
                self.timers_pending = self.timers_pending.saturating_sub(1);
                // Otherwise a later timer superseded this one.
                if self.timers_pending == 0 {
                    self.log(LogLevel::Trace, format_args!("Timer fired."));
                    self.rechecks_left = self.rechecks_left.saturating_sub(1);
                    // A changed command resets `rechecks_left` and schedules
                    // the next check itself.
                    self.recheck(host);
                    if self.rechecks_left > 0 {
                        self.schedule_recheck(host);
                    }
                } else {
                    self.log(LogLevel::Trace, format_args!("Timer ignored as stale."));
                }
            }

            // Zellij broadcasts this to every client's plugin instance, for any
            // terminal pane whose foreground command changed since its last
            // round (about once a second, and only for panes that printed
            // something). Only the pane this client has focused matters. The
            // event never moves the focus cache: a late event for a pane the
            // client just left must not pull focus back to it.
            Event::CommandChanged(pane, argv, is_foreground, _focused_client_ids)
                if self.is_active() && self.focus.pane == Some(pane) =>
            {
                // `is_foreground` is false both for a shell back at its prompt
                // and for a command pane running its program, so it cannot mean
                // "idle". It is only logged.
                self.log(
                    LogLevel::Debug,
                    format_args!("Command changed in {pane:?} (foreground: {is_foreground})"),
                );
                self.assess(argv, host);
                // The ticker has seen this command, so it will report the next
                // change itself. One more check is enough, to correct an event
                // that is older than the last query.
                self.rechecks_left = self.rechecks_left.min(1);
            }

            Event::PluginConfigurationChanged(configuration) => {
                self.apply_configuration_change(configuration, host);
            }

            _ => {}
        }
        false // No need to render UI.
    }

    fn handle_pipe(&mut self, pipe_message: PipeMessage, host: &mut impl Host) -> bool {
        self.log(
            LogLevel::Trace,
            format_args!(
                "Pipe {:?} received with payload {:?}",
                pipe_message.name, pipe_message.payload
            ),
        );
        let was_enabled = self.is_enabled;

        if let Some(action) = pipe_message.payload.as_deref() {
            self.is_enabled = match action {
                "enable" => true,
                "disable" => false,
                "toggle" => !self.is_enabled,
                other => {
                    self.log(LogLevel::Warn, format_args!(
                        "Unknown pipe payload {other:?}; expected `enable`, `disable`, or `toggle`."
                    ));
                    self.is_enabled
                }
            };
            self.log(LogLevel::Info, format_args!("Enabled: {}", self.is_enabled));
        }

        if self.is_enabled {
            if !was_enabled {
                // Nothing was tracked while disabled, so start from scratch:
                // the focused pane is assessed even if its command is the
                // one last seen before disabling.
                self.focus = Focus::default();
            }
            self.recheck(host);
            self.schedule_recheck(host);
        }

        false // No need to render UI.
    }

    /// Whether the plugin may query Zellij and act on the answers. Queries
    /// without `ReadApplicationState` panic (see `Host`), and a disabled
    /// plugin leaves Zellij alone.
    fn is_active(&self) -> bool {
        self.permissions_granted && self.is_enabled
    }

    /// Look at the focused pane after a possible focus change: re-query which
    /// pane is focused, and assess it only if it has not been assessed yet.
    fn refresh_focus(&mut self, host: &mut impl Host) {
        if !self.is_active() {
            return;
        }
        self.query_focus(host);
        if self.focus.command.is_none() {
            self.assess_focused_pane(host);
        }
    }

    /// Look at the focused pane again whether or not focus changed, in case
    /// the command running in it did.
    fn recheck(&mut self, host: &mut impl Host) {
        if !self.is_active() {
            return;
        }
        self.query_focus(host);
        self.assess_focused_pane(host);
    }

    /// Ask Zellij which pane the client has focused. A different pane forgets
    /// the cached command; a failed query keeps the cache as it is.
    fn query_focus(&mut self, host: &mut impl Host) {
        match host.focused_pane() {
            Ok(pane) if self.focus.pane != Some(pane) => {
                self.log(LogLevel::Debug, format_args!("Focused pane: {pane:?}"));
                self.focus = Focus {
                    pane: Some(pane),
                    command: None,
                };
            }
            Ok(_) => {}
            Err(error) => self.log(
                LogLevel::Error,
                format_args!("Could not get the focused pane: {error}"),
            ),
        }
    }

    /// Query what the focused pane runs and assess it. Does nothing while the
    /// focused pane is unknown.
    fn assess_focused_pane(&mut self, host: &mut impl Host) {
        let Some(pane) = self.focus.pane else {
            return;
        };

        let argv = match pane {
            PaneId::Terminal(_) => match host.pane_command(pane) {
                Ok(argv) => argv,
                Err(error) => {
                    // Not the same as "nothing is running": the pane may be
                    // closing, or the query timed out. Keep the current mode.
                    self.log(
                        LogLevel::Error,
                        format_args!("Could not get the command of {pane:?}: {error}"),
                    );
                    return;
                }
            },
            // A plugin pane runs no command. Its location goes through the
            // same rules instead, as `list_clients` used to report it.
            PaneId::Plugin(_) => match host.plugin_url(pane) {
                Some(url) => vec![url],
                None => {
                    self.log(
                        LogLevel::Error,
                        format_args!("Could not get the location of {pane:?}"),
                    );
                    return;
                }
            },
        };

        self.assess(argv, host);
    }

    /// Act on the focused pane's command if it differs from the one last
    /// assessed. An unchanged command is left alone, so a mode the user picked
    /// by hand sticks until the command, or the focused pane, changes.
    fn assess(&mut self, argv: Vec<String>, host: &mut impl Host) {
        if self.focus.command.as_ref() == Some(&argv) {
            return;
        }

        let target_input_mode = self.determine_target_mode(&argv);

        // Only switch if the mode is actually changing, and
        // if the current input mode is `Locked` or `Normal`
        if self.current_mode != target_input_mode
            && (self.current_mode == InputMode::Locked || self.current_mode == InputMode::Normal)
        {
            self.log(
                LogLevel::Info,
                format_args!(
                    "Switching to {target_input_mode:?} for `{}`",
                    executable(&argv)
                ),
            );
            host.switch_to_input_mode(target_input_mode);
        }
        self.focus.command = Some(argv);

        // The command changed, so keep looking for a while in case it
        // changes again before Zellij's ticker notices.
        self.rechecks_left = RECHECK_ROUNDS;
        self.schedule_recheck(host);
    }

    /// Check the focused pane after `RECHECK_DELAY_SECONDS`, unless a check is
    /// already pending.
    fn schedule_recheck(&mut self, host: &mut impl Host) {
        if self.timers_pending == 0 {
            self.restart_recheck(host);
        }
    }

    /// Check the focused pane after `RECHECK_DELAY_SECONDS`, superseding any
    /// check already pending.
    fn restart_recheck(&mut self, host: &mut impl Host) {
        if self.is_enabled {
            host.set_timeout(RECHECK_DELAY_SECONDS);
            self.timers_pending += 1;
        }
    }

    /// Decide the mode for a pane running `argv`. Each rule is tested against
    /// the whole command line and against the `executable`.
    fn determine_target_mode(&self, argv: &[String]) -> InputMode {
        let command_line = argv.join(" ");
        let executable = executable(argv);

        let texts = [command_line.as_str(), executable];
        let lock = is_match(self.lock_regex.as_ref(), texts)
            || is_match(self.lock_triggers_deprecated.as_ref(), texts);
        let ignore = is_match(self.ignore_regex.as_ref(), texts);

        let target = if lock && !ignore {
            InputMode::Locked
        } else {
            InputMode::Normal
        };

        self.log(
            LogLevel::Debug,
            format_args!("Assessed `{command_line}` (executable `{executable}`): {target:?}"),
        );

        target
    }
}

/// The executable of `argv`: the last path segment of its first argument,
/// after `/` or Windows' `\`, with any surrounding parentheses removed
/// (fish reports `(atuin)`). The first argument is taken whole, so a path
/// containing spaces stays intact.
fn executable(argv: &[String]) -> &str {
    argv.first()
        .and_then(|first| first.split(['/', '\\']).next_back())
        .unwrap_or("")
        .trim_matches(['(', ')'])
}

/// The variant name of an event, for trace logging without its payload.
fn event_name(event: &Event) -> &'static str {
    match event {
        Event::PermissionRequestResult(_) => "PermissionRequestResult",
        Event::InputReceived => "InputReceived",
        Event::ModeUpdate(_) => "ModeUpdate",
        Event::TabUpdate(_) => "TabUpdate",
        Event::PaneUpdate(_) => "PaneUpdate",
        Event::Timer(_) => "Timer",
        Event::CommandChanged(..) => "CommandChanged",
        Event::PluginConfigurationChanged(_) => "PluginConfigurationChanged",
        _ => "other",
    }
}
