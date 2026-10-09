//! Native unit tests for the plugin's event logic.
//!
//! These run on the host target (`just test`), not in wasm. Zellij is replaced
//! by `MockHost`, which records every call the plugin makes so each test can
//! assert exactly which host actions a sequence of events produced. Queries
//! are recorded too, and answered from a small world each test scripts up
//! front (`focus`, `running`, `failing`, `plugin`).
//!
//! Tests are grouped into `decision`, `config`, `events`, `command_changed`,
//! and `pipes`, so one group can be run alone with e.g. `just test events::`.

use super::*;
use std::collections::HashMap;

#[derive(Debug, PartialEq)]
enum HostCall {
    FocusedPane,
    PaneCommand(PaneId),
    PluginUrl(PaneId),
    SetTimeout(f64),
    SwitchToInputMode(InputMode),
    HideSelf,
}

/// A recording stand-in for Zellij that answers queries from a scripted world.
///
/// Anything not scripted answers the way Zellij does for a missing pane: an
/// error for `focused_pane` and `pane_command`, `None` for `plugin_url`.
#[derive(Default)]
struct MockHost {
    calls: Vec<HostCall>,
    /// The client's focused pane.
    focused: Option<PaneId>,
    /// Per-pane answers to `pane_command`.
    commands: HashMap<PaneId, Result<Vec<String>, String>>,
    /// Per-pane answers to `plugin_url`.
    plugin_urls: HashMap<PaneId, String>,
}

impl MockHost {
    /// Return the calls recorded so far and clear the log.
    fn drain(&mut self) -> Vec<HostCall> {
        std::mem::take(&mut self.calls)
    }
}

// Scripting helpers. They return `&mut Self` so a world can be set up in one
// chain, e.g. `host.focus(pane).running(pane, &["nvim", "main.rs"])`.
impl MockHost {
    /// Make `pane` the client's focused pane.
    fn focus(&mut self, pane: PaneId) -> &mut Self {
        self.focused = Some(pane);
        self
    }

    /// Make `pane` report `argv` as its running command.
    fn running(&mut self, pane: PaneId, argv: &[&str]) -> &mut Self {
        let argv = argv.iter().map(ToString::to_string).collect();
        self.commands.insert(pane, Ok(argv));
        self
    }

    /// Make the command query for `pane` fail, as for a closing pane or a timeout.
    fn failing(&mut self, pane: PaneId) -> &mut Self {
        let error = format!("Could not retrieve running command for pane {pane:?}");
        self.commands.insert(pane, Err(error));
        self
    }

    /// Make `pane` a plugin pane loaded from `url`.
    fn plugin(&mut self, pane: PaneId, url: &str) -> &mut Self {
        self.plugin_urls.insert(pane, url.to_string());
        self
    }
}

impl Host for MockHost {
    fn focused_pane(&mut self) -> Result<PaneId, String> {
        self.calls.push(HostCall::FocusedPane);
        self.focused
            .ok_or_else(|| "No active pane found for client".to_string())
    }

    fn pane_command(&mut self, pane: PaneId) -> Result<Vec<String>, String> {
        self.calls.push(HostCall::PaneCommand(pane));
        self.commands
            .get(&pane)
            .cloned()
            .unwrap_or_else(|| Err(format!("Terminal pane {pane:?} not found or not running")))
    }

    fn plugin_url(&mut self, pane: PaneId) -> Option<String> {
        self.calls.push(HostCall::PluginUrl(pane));
        self.plugin_urls.get(&pane).cloned()
    }

    fn set_timeout(&mut self, seconds: f64) {
        self.calls.push(HostCall::SetTimeout(seconds));
    }

    fn switch_to_input_mode(&mut self, mode: InputMode) {
        self.calls.push(HostCall::SwitchToInputMode(mode));
    }

    fn hide_self(&mut self) {
        self.calls.push(HostCall::HideSelf);
    }
}

// --- Builders (shared by every module below) --------------------------------

const PANE_1: PaneId = PaneId::Terminal(1);
const PANE_2: PaneId = PaneId::Terminal(2);
const PLUGIN_PANE: PaneId = PaneId::Plugin(1);

/// The source pattern of a compiled rule, for asserting on configuration.
fn pattern(re: &Option<Regex>) -> Option<&str> {
    re.as_ref().map(Regex::as_str)
}

fn state_with(config: &[(&str, &str)]) -> State {
    let mut state = State::default();
    state.load_configuration(
        config
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    );
    state
}

/// A plugin loaded with `config` and granted its permissions, with its first
/// look at the focused pane (and the follow-up checks it arms) already done
/// and the resulting host calls drained. Script the world before calling.
fn started(config: &[(&str, &str)], host: &mut MockHost) -> State {
    let mut state = state_with(config);
    state.handle_event(granted(), host);
    settle(&mut state, host);
    host.drain();
    state
}

/// Fire timers until none is pending, as Zellij would once the world stops
/// changing: the follow-up checks after a change run out.
fn settle(state: &mut State, host: &mut MockHost) {
    // Bounded, so a timer that keeps re-arming fails the test instead of hanging.
    for _ in 0..100 {
        if state.timers_pending == 0 {
            return;
        }
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), host);
    }
    panic!("timers never settled");
}

/// The `lock_regex` / `ignore_regex` values from the README's example
/// configuration, read from the file so tests stay tied to what users are
/// told to run.
fn readme_config() -> Vec<(&'static str, String)> {
    const README: &str = include_str!("../README.md");
    ["lock_regex", "ignore_regex"]
        .into_iter()
        .map(|key| {
            let prefix = format!("{key} \"");
            let line = README
                .lines()
                .map(str::trim_start)
                .find(|line| line.starts_with(&prefix))
                .unwrap_or_else(|| panic!("README example config has no `{key}` line"));
            let value = line[prefix.len()..]
                .strip_suffix('"')
                .unwrap_or_else(|| panic!("`{key}` is not a quoted string: {line:?}"));
            // KDL string escapes; the README only uses `\\`.
            (key, value.replace("\\\\", "\\"))
        })
        .collect()
}

/// The mode `state` picks for a pane running `command_line`, split into argv
/// on whitespace.
fn mode_for(state: &State, command_line: &str) -> InputMode {
    let argv: Vec<String> = command_line
        .split_whitespace()
        .map(str::to_string)
        .collect();
    state.determine_target_mode(&argv)
}

fn granted() -> Event {
    Event::PermissionRequestResult(PermissionStatus::Granted)
}

fn mode_update(mode: InputMode) -> Event {
    Event::ModeUpdate(ModeInfo {
        mode,
        ..Default::default()
    })
}

/// A `TabUpdate`. The plugin asks Zellij for the focused pane instead of
/// reading the payload, so it is left empty.
fn tab_update() -> Event {
    Event::TabUpdate(Vec::new())
}

/// A `PaneUpdate`. The plugin asks Zellij for the focused pane instead of
/// reading the payload, so it is left empty.
fn pane_update() -> Event {
    Event::PaneUpdate(PaneManifest::default())
}

/// A `CommandChanged` event for `pane`. Every event claims that client 1 has
/// the pane focused, so tests show the plugin does not rely on that list.
fn command_changed(pane: PaneId, argv: &[&str], is_foreground: bool) -> Event {
    let argv = argv.iter().map(ToString::to_string).collect();
    Event::CommandChanged(pane, argv, is_foreground, vec![1])
}

/// A `PluginConfigurationChanged` event carrying the whole new `config` block.
fn config_changed(config: &[(&str, &str)]) -> Event {
    Event::PluginConfigurationChanged(
        config
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    )
}

fn pipe(payload: Option<&str>) -> PipeMessage {
    PipeMessage {
        source: PipeSource::Keybind,
        name: "autolock".to_string(),
        payload: payload.map(str::to_string),
        args: BTreeMap::new(),
        is_private: true,
    }
}

/// `determine_target_mode` and the rules that feed it.
mod decision {
    use super::*;

    #[test]
    fn default_config_locks_everything_but_shells() {
        let state = State::default();
        // An idle pane reports its shell, so this is the idle case.
        assert_eq!(mode_for(&state, "/bin/zsh"), InputMode::Normal);
        for cmd in [
            "htop",
            "vim",
            "nvim ~/notes.md",
            "less README.md",
            "sudo vim /etc/hosts",
            "python3",
            "cat vim.txt",
        ] {
            assert_eq!(mode_for(&state, cmd), InputMode::Locked, "{cmd:?}");
        }
    }

    #[test]
    fn default_config_leaves_shells_and_idle_panes_unlocked() {
        let state = State::default();
        for cmd in [
            "",
            "sh",
            "dash",
            "bash -l",
            "/bin/zsh",
            "/usr/bin/fish",
            "ksh",
            "tcsh",
            "nu",
            "xonsh",
            "elvish",
            "pwsh -NoLogo",
            "zellij",
            "ls -la",
            "lsd --tree",
            "eza --tree",
            "starship prompt",
            "direnv export zsh",
            "(atuin)",
            "atuin history start -- ls",
        ] {
            assert_eq!(mode_for(&state, cmd), InputMode::Normal, "{cmd:?}");
        }
    }

    #[test]
    fn default_config_leaves_windows_shells_unlocked() {
        // The executable keeps its extension, and Windows paths may contain spaces.
        let state = State::default();
        for exe in [
            r"C:\Windows\System32\cmd.exe",
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            r"C:\Program Files\PowerShell\7\pwsh.exe",
            r"C:\Program Files\Git\bin\bash.exe",
        ] {
            let argv = [exe.to_string()];
            assert_eq!(
                state.determine_target_mode(&argv),
                InputMode::Normal,
                "{exe}"
            );
        }
        let argv = [r"C:\Program Files\Neovim\bin\nvim.exe".to_string()];
        assert_eq!(state.determine_target_mode(&argv), InputMode::Locked);
    }

    #[test]
    fn executable_is_extracted_from_absolute_path() {
        let state = state_with(&[("lock_regex", "^nvim$")]);
        assert_eq!(
            mode_for(&state, "/usr/local/bin/nvim --clean"),
            InputMode::Locked
        );
    }

    #[test]
    fn executable_is_the_whole_first_argument_even_with_spaces() {
        // Zellij reports argv, so a path with spaces arrives as one argument.
        // Splitting the joined command line on whitespace would yield `/opt/My`.
        let state = state_with(&[("lock_regex", "^nvim$")]);
        let argv = ["/opt/My Tools/nvim", "notes.md"].map(str::to_string);
        assert_eq!(state.determine_target_mode(&argv), InputMode::Locked);
    }

    #[test]
    fn executable_is_extracted_from_windows_path_with_spaces() {
        // Issue #19: `C:\Program Files\...` used to yield `C:\Program`.
        let argv = [r"C:\Program Files\Neovim\bin\nvim.exe", "notes.md"].map(str::to_string);

        // The executable is `nvim.exe`, so an anchored pattern has to allow
        // for the extension.
        let state = state_with(&[("lock_regex", r"^nvim(\.exe)?$")]);
        assert_eq!(state.determine_target_mode(&argv), InputMode::Locked);
        let state = state_with(&[("lock_regex", "^nvim$")]);
        assert_eq!(state.determine_target_mode(&argv), InputMode::Normal);
    }

    #[test]
    fn executable_is_extracted_from_parenthesised_process_name() {
        // Some shells report background/child processes as `(name)`.
        let state = state_with(&[("lock_regex", "^atuin$")]);
        assert_eq!(mode_for(&state, "(atuin)"), InputMode::Locked);
        assert_eq!(
            mode_for(&state, "/usr/bin/atuin search -i"),
            InputMode::Locked
        );
    }

    #[test]
    fn lock_regex_is_also_tested_against_full_command_line() {
        let state = state_with(&[("lock_regex", "^ssh .*prod")]);
        assert_eq!(mode_for(&state, "ssh deploy@prod-1"), InputMode::Locked);
        assert_eq!(mode_for(&state, "ssh deploy@staging"), InputMode::Normal);
    }

    #[test]
    fn ignore_regex_overrides_lock_regex() {
        let state = state_with(&[
            ("lock_regex", ".*"),
            ("ignore_regex", "^(zellij|atuin history start.*)$"),
        ]);
        // Tested against the full command line as well as the executable.
        assert_eq!(mode_for(&state, "zellij"), InputMode::Normal);
        assert_eq!(
            mode_for(&state, "atuin history start -- ls"),
            InputMode::Normal
        );
        // Sanity: the catch-all lock regex still locks anything else.
        assert_eq!(mode_for(&state, "htop"), InputMode::Locked);
    }

    #[test]
    fn custom_ignore_regex_replaces_default() {
        let state = state_with(&[("lock_regex", ".*"), ("ignore_regex", "^htop$")]);
        assert_eq!(mode_for(&state, "htop"), InputMode::Normal);
        // The default list is not added to it, so `zellij` and shells lock.
        assert_eq!(mode_for(&state, "zellij"), InputMode::Locked);
        assert_eq!(mode_for(&state, "/bin/zsh"), InputMode::Locked);
    }

    #[test]
    fn invalid_regex_fails_closed() {
        let state = state_with(&[("lock_regex", "(unclosed")]);
        assert_eq!(mode_for(&state, "vim"), InputMode::Normal);

        // An invalid ignore regex must not accidentally suppress locking either.
        let state = state_with(&[("lock_regex", ".*"), ("ignore_regex", "[")]);
        assert_eq!(mode_for(&state, "vim"), InputMode::Locked);
    }

    #[test]
    fn empty_lock_regex_never_locks() {
        let state = state_with(&[("lock_regex", "")]);
        assert_eq!(pattern(&state.lock_regex), None);
        assert_eq!(mode_for(&state, "vim"), InputMode::Normal);
    }

    #[test]
    fn empty_and_invalid_patterns_are_dropped_at_load() {
        let state = state_with(&[
            ("lock_regex", "(unclosed"),
            ("ignore_regex", ""),
            ("triggers", "["),
        ]);
        assert_eq!(pattern(&state.lock_regex), None);
        assert_eq!(pattern(&state.ignore_regex), None);
        assert_eq!(pattern(&state.lock_triggers_deprecated), None);
    }

    #[test]
    fn catch_all_lock_regex_never_locks_an_empty_command() {
        let state = state_with(&[("lock_regex", ".*")]);
        assert_eq!(mode_for(&state, ""), InputMode::Normal);
        assert_eq!(mode_for(&state, "htop"), InputMode::Locked);
    }

    #[test]
    fn idle_shell_is_matched_like_any_other_command() {
        // A pane at its prompt reports the shell's own argv, not "no command"
        // (a background plugin has no way to learn Zellij's default shell).
        // A catch-all `lock_regex` therefore needs the shell in `ignore_regex`.
        let state = state_with(&[("lock_regex", ".*")]);
        assert_eq!(mode_for(&state, "/bin/zsh"), InputMode::Locked);

        let state = state_with(&[("lock_regex", ".*"), ("ignore_regex", "^zsh$")]);
        assert_eq!(mode_for(&state, "/bin/zsh"), InputMode::Normal);
    }

    #[test]
    fn readme_example_config_shows_the_defaults() {
        // The README tells users to extend the default `ignore_regex`, so the
        // example has to be exactly that.
        let config = readme_config();
        let config: Vec<(&str, &str)> = config.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let state = state_with(&config);
        let defaults = State::default();
        assert_eq!(pattern(&state.lock_regex), pattern(&defaults.lock_regex));
        assert_eq!(
            pattern(&state.ignore_regex),
            pattern(&defaults.ignore_regex)
        );
    }

    #[test]
    fn readme_recommended_config_locks_everything_but_the_shell() {
        let config = readme_config();
        let config: Vec<(&str, &str)> = config.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let state = state_with(&config);
        assert_eq!(pattern(&state.lock_regex), Some(".*"));

        // Shells (idle ones report their own path), prompt tooling, and a pane
        // with no argv stay unlocked.
        for cmd in [
            "",
            "zsh",
            "/bin/zsh",
            "zsh -l",
            "bash",
            "/usr/bin/fish",
            "ls -la",
            "eza --tree",
            "(atuin)",
            "atuin history start -- ls",
            "starship prompt",
            "direnv export zsh",
        ] {
            assert_eq!(mode_for(&state, cmd), InputMode::Normal, "{cmd:?}");
        }

        // Anything interactive locks without being enumerated.
        for cmd in [
            "htop",
            "ssh host",
            "sudo vim /etc/hosts",
            "nvim main.rs",
            "less README.md",
            "python3",
        ] {
            assert_eq!(mode_for(&state, cmd), InputMode::Locked, "{cmd:?}");
        }
    }

    #[test]
    fn deprecated_triggers_are_anchored_whole_word_alternatives() {
        let state = state_with(&[("lock_regex", ""), ("triggers", "htop|less")]);
        assert_eq!(mode_for(&state, "htop"), InputMode::Locked);
        assert_eq!(mode_for(&state, "less README.md"), InputMode::Locked);
        // Anchored: a prefix match is not enough.
        assert_eq!(mode_for(&state, "htopx"), InputMode::Normal);
    }
}

/// `load_configuration` and runtime configuration changes: defaults,
/// overrides, and value parsing.
mod config {
    use super::*;

    #[test]
    fn defaults_when_configuration_is_empty() {
        let state = state_with(&[]);
        assert!(state.is_enabled);
        assert_eq!(state.log_level, LogLevel::Info);
        assert_eq!(pattern(&state.lock_regex), Some(".*"));
        assert_eq!(pattern(&state.ignore_regex), Some(DEFAULT_IGNORE_REGEX));
        assert_eq!(pattern(&state.lock_triggers_deprecated), None);
    }

    #[test]
    fn setting_any_rule_drops_every_rule_default() {
        // `lock_regex` alone: no default shell list, so an idle shell locks.
        let state = state_with(&[("lock_regex", ".*")]);
        assert_eq!(pattern(&state.ignore_regex), None);
        assert_eq!(mode_for(&state, "/bin/zsh"), InputMode::Locked);

        // `ignore_regex` alone: no catch-all, so nothing locks.
        let state = state_with(&[("ignore_regex", "^zsh$")]);
        assert_eq!(pattern(&state.lock_regex), None);
        assert_eq!(mode_for(&state, "htop"), InputMode::Normal);

        // Empty values count as set: this block locks nothing.
        let state = state_with(&[("lock_regex", ""), ("ignore_regex", "")]);
        assert_eq!(pattern(&state.lock_regex), None);
        assert_eq!(pattern(&state.ignore_regex), None);

        // Other keys keep the rule defaults.
        let state = state_with(&[("is_enabled", "true"), ("log_level", "info")]);
        assert_eq!(pattern(&state.lock_regex), Some(DEFAULT_LOCK_REGEX));
        assert_eq!(pattern(&state.ignore_regex), Some(DEFAULT_IGNORE_REGEX));
    }

    #[test]
    fn triggers_without_lock_regex_do_not_lock_everything() {
        // A 0.2 config lists what to lock in `triggers` alone. The catch-all
        // default must not turn that allowlist into "lock everything".
        let state = state_with(&[("triggers", "vim|htop")]);
        assert_eq!(pattern(&state.lock_regex), None);
        assert_eq!(pattern(&state.ignore_regex), None);
        assert_eq!(mode_for(&state, "vim notes.md"), InputMode::Locked);
        assert_eq!(mode_for(&state, "python3"), InputMode::Normal);
        assert_eq!(mode_for(&state, "zsh"), InputMode::Normal);

        // Setting both combines them, as documented.
        let state = state_with(&[("triggers", "vim"), ("lock_regex", "^htop$")]);
        assert_eq!(mode_for(&state, "vim"), InputMode::Locked);
        assert_eq!(mode_for(&state, "htop"), InputMode::Locked);
        assert_eq!(mode_for(&state, "python3"), InputMode::Normal);
    }

    #[test]
    fn changing_to_a_triggers_only_block_drops_the_rule_defaults() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);
        assert_eq!(pattern(&state.lock_regex), Some(".*"));

        state.handle_event(config_changed(&[("triggers", "vim")]), &mut host);
        assert_eq!(pattern(&state.lock_regex), None);
        assert_eq!(pattern(&state.ignore_regex), None);

        // Removing `triggers` again brings the defaults back.
        state.handle_event(config_changed(&[]), &mut host);
        assert_eq!(pattern(&state.lock_regex), Some(".*"));
        assert_eq!(pattern(&state.ignore_regex), Some(DEFAULT_IGNORE_REGEX));
    }

    #[test]
    fn configuration_overrides_defaults() {
        let state = state_with(&[
            ("is_enabled", "false"),
            ("lock_regex", "^(vim|htop)$"),
            ("ignore_regex", "^zellij$"),
            ("triggers", "less|more"),
            ("log_level", "warn"),
        ]);
        assert!(!state.is_enabled);
        assert_eq!(state.log_level, LogLevel::Warn);
        assert_eq!(pattern(&state.lock_regex), Some("^(vim|htop)$"));
        assert_eq!(pattern(&state.ignore_regex), Some("^zellij$"));
        assert_eq!(
            pattern(&state.lock_triggers_deprecated),
            Some("^(less|more)$")
        );
    }

    #[test]
    fn log_level_is_parsed_case_insensitively() {
        for (name, level) in [
            ("trace", LogLevel::Trace),
            ("DEBUG", LogLevel::Debug),
            ("Info", LogLevel::Info),
            (" warn ", LogLevel::Warn),
            ("error", LogLevel::Error),
            ("critical", LogLevel::Critical),
        ] {
            let state = state_with(&[("log_level", name)]);
            assert_eq!(state.log_level, level, "{name:?}");
        }
    }

    #[test]
    fn unrecognised_log_level_keeps_the_default() {
        let state = state_with(&[("log_level", "verbose")]);
        assert_eq!(state.log_level, LogLevel::Info);
        let state = state_with(&[("log_level", "")]);
        assert_eq!(state.log_level, LogLevel::Info);
    }

    #[test]
    fn deprecated_print_to_log_true_means_debug() {
        let state = state_with(&[("print_to_log", "true")]);
        assert_eq!(state.log_level, LogLevel::Debug);
        let state = state_with(&[("print_to_log", "false")]);
        assert_eq!(state.log_level, LogLevel::Info);
    }

    #[test]
    fn log_level_beats_deprecated_print_to_log() {
        let state = state_with(&[("print_to_log", "true"), ("log_level", "error")]);
        assert_eq!(state.log_level, LogLevel::Error);
        let state = state_with(&[("log_level", "error"), ("print_to_log", "true")]);
        assert_eq!(state.log_level, LogLevel::Error);

        // An unrecognised `log_level` leaves whatever the alias set in place.
        let state = state_with(&[("print_to_log", "true"), ("log_level", "loud")]);
        assert_eq!(state.log_level, LogLevel::Debug);
    }

    #[test]
    fn boolean_config_accepts_documented_spellings_only() {
        for truthy in ["true", "t", "y", "1", " true "] {
            assert!(parse_bool_config(truthy), "{truthy:?}");
        }
        for falsy in ["false", "f", "n", "0", "", "yes", "TRUE", "on"] {
            assert!(!parse_bool_config(falsy), "{falsy:?}");
        }
    }

    #[test]
    fn configuration_change_reloads_patterns_and_reassesses() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["htop"]);
        let mut state = started(&[("lock_regex", "^vim$")], &mut host);

        // htop did not lock. Under the new rules it does, so the unchanged
        // command is assessed again.
        state.handle_event(config_changed(&[("lock_regex", "^(vim|htop)$")]), &mut host);
        assert_eq!(pattern(&state.lock_regex), Some("^(vim|htop)$"));
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn unchanged_configuration_block_is_ignored() {
        // Zellij compares a reloaded block with the one the plugin was loaded
        // with, so once it has changed, every later reload sends it again.
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["htop"]);
        let mut state = started(&[("lock_regex", "^vim$")], &mut host);

        // The block the plugin was loaded with.
        state.handle_event(config_changed(&[("lock_regex", "^vim$")]), &mut host);
        assert_eq!(host.drain(), vec![]);

        // A real change locks; the user then unlocks by hand.
        let changed = [("lock_regex", "^(vim|htop)$")];
        state.handle_event(config_changed(&changed), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(mode_update(InputMode::Normal), &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        host.drain();

        // The same block again, e.g. after saving an unrelated setting, must
        // not assess again and undo the manual unlock.
        state.handle_event(config_changed(&changed), &mut host);
        assert_eq!(host.drain(), vec![]);
    }

    #[test]
    fn keys_removed_by_a_change_revert_to_their_defaults() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(
            &[
                ("lock_regex", "^htop$"),
                ("ignore_regex", ""),
                ("triggers", "less"),
            ],
            &mut host,
        );

        state.handle_event(config_changed(&[]), &mut host);

        let defaults = State::default();
        assert_eq!(pattern(&state.lock_regex), pattern(&defaults.lock_regex));
        assert_eq!(
            pattern(&state.ignore_regex),
            pattern(&defaults.ignore_regex)
        );
        assert_eq!(pattern(&state.lock_triggers_deprecated), None);
    }

    #[test]
    fn change_keeps_a_pipe_choice_unless_it_edits_is_enabled() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);
        state.handle_pipe(pipe(Some("disable")), &mut host);
        host.running(PANE_1, &["vim"]);

        // A change to another setting keeps the plugin disabled.
        state.handle_event(config_changed(&[("lock_regex", "^(vim|htop)$")]), &mut host);
        assert!(!state.is_enabled);
        assert_eq!(host.drain(), vec![]);

        // A change that edits `is_enabled` itself wins, and starts from scratch.
        state.handle_event(
            config_changed(&[("lock_regex", "^(vim|htop)$"), ("is_enabled", "true")]),
            &mut host,
        );
        assert!(state.is_enabled);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }
}

/// `handle_event`: permissions, timers, focus tracking, and mode switches.
mod events {
    use super::*;

    #[test]
    fn hides_itself_and_assesses_the_focused_pane_once_permissions_are_granted() {
        let mut state = State::default();
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);

        state.handle_event(
            Event::PermissionRequestResult(PermissionStatus::Denied),
            &mut host,
        );
        assert!(!state.permissions_granted);
        assert_eq!(host.drain(), vec![]);

        state.handle_event(granted(), &mut host);
        assert!(state.permissions_granted);
        // A first assessment always counts as a change, so it arms a follow-up.
        assert_eq!(
            host.drain(),
            vec![
                HostCall::HideSelf,
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn no_queries_before_permission_is_granted() {
        // Zellij never answers a query the plugin lacks permission for, and
        // the shim panics waiting. Nothing may ask before the grant.
        let mut state = State::default();
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["vim"]);

        state.handle_event(
            Event::PermissionRequestResult(PermissionStatus::Denied),
            &mut host,
        );
        state.handle_event(tab_update(), &mut host);
        state.handle_event(pane_update(), &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        state.handle_event(mode_update(InputMode::Scroll), &mut host);
        state.handle_event(mode_update(InputMode::Normal), &mut host);
        state.handle_pipe(pipe(None), &mut host);

        // Only the pipe's timer is armed; it needs no permission.
        assert_eq!(
            host.drain(),
            vec![HostCall::SetTimeout(RECHECK_DELAY_SECONDS)]
        );
    }

    #[test]
    fn input_pushes_back_a_pending_check() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);

        // Typing `nvim`, then `Enter`. Timers cannot be cancelled, so each key
        // sets one, and the first to fire is superseded: no check yet.
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );

        // The last one checks, after the editor has started.
        host.running(PANE_1, &["nvim"]);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn rechecks_stop_when_the_window_ends() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);
        host.running(PANE_1, &["vim"]);
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        host.drain();

        // The change starts `RECHECK_ROUNDS` checks; each finds the same
        // command and arms the next, until the last.
        for _ in 1..RECHECK_ROUNDS {
            state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
            assert_eq!(
                host.drain(),
                vec![
                    HostCall::FocusedPane,
                    HostCall::PaneCommand(PANE_1),
                    HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
                ]
            );
        }
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::FocusedPane, HostCall::PaneCommand(PANE_1)]
        );
        assert_eq!(state.timers_pending, 0);
    }

    #[test]
    fn short_command_that_exits_unseen_by_zellij_still_unlocks() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);

        // `sleep 0.8⏎`: the check after `Enter` sees it and locks.
        host.running(PANE_1, &["sleep", "0.8"]);
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        host.drain();

        // Still running at the next check.
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );

        // It exits before Zellij's ticker ever saw it, so no `CommandChanged`
        // follows. A later check in the window unlocks.
        host.running(PANE_1, &["zsh"]);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SwitchToInputMode(InputMode::Normal),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn mode_updates_do_not_schedule_a_check() {
        let mut state = State::default();
        let mut host = MockHost::default();

        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(mode_update(InputMode::Normal), &mut host);

        assert_eq!(state.current_mode, InputMode::Normal);
        assert_eq!(host.drain(), vec![]);
    }

    #[test]
    fn removed_reaction_seconds_key_is_ignored() {
        // 0.2 configs may still set it; the delay is no longer configurable.
        let mut state = state_with(&[("reaction_seconds", "0.05")]);
        let mut host = MockHost::default();

        state.handle_event(Event::InputReceived, &mut host);

        assert_eq!(
            host.drain(),
            vec![HostCall::SetTimeout(RECHECK_DELAY_SECONDS)]
        );
    }

    #[test]
    fn timer_rechecks_the_focused_pane_and_allows_rearming() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);

        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        // Same pane, same command: nothing to do, and no follow-up.
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
            ]
        );

        // The timer fired, so new input may schedule another one.
        state.handle_event(Event::InputReceived, &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::SetTimeout(RECHECK_DELAY_SECONDS)]
        );
    }

    #[test]
    fn locks_when_editor_starts_and_unlocks_when_it_exits() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);

        // Editor starts: switch to Locked and look again shortly after.
        host.running(PANE_1, &["nvim", "main.rs"]);
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );

        // Zellij confirms the switch; the follow-up checks find the same
        // command and run out.
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        settle(&mut state, &mut host);
        host.drain();

        // Editor exits back to the shell (`:q⏎`): switch to Normal.
        host.running(PANE_1, &["zsh"]);
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SwitchToInputMode(InputMode::Normal),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn does_not_switch_when_already_in_target_mode() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);

        // The command changed, so a follow-up check is armed, but no switch.
        host.running(PANE_1, &["vim"]);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn never_overrides_modes_other_than_normal_and_locked() {
        for mode in [
            InputMode::Pane,
            InputMode::Tab,
            InputMode::Scroll,
            InputMode::Search,
            InputMode::RenameTab,
        ] {
            let mut host = MockHost::default();
            host.focus(PANE_1).running(PANE_1, &["zsh"]);
            let mut state = started(&[], &mut host);
            state.handle_event(mode_update(mode), &mut host);

            // The command changed, so a follow-up check is armed, but no switch.
            host.running(PANE_1, &["vim"]);
            state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
            assert_eq!(
                host.drain(),
                vec![
                    HostCall::FocusedPane,
                    HostCall::PaneCommand(PANE_1),
                    HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
                ],
                "{mode:?}"
            );

            // Back in Normal mode, the pane is looked at afresh.
            state.handle_event(mode_update(InputMode::Normal), &mut host);
            assert_eq!(
                host.drain(),
                vec![
                    HostCall::FocusedPane,
                    HostCall::PaneCommand(PANE_1),
                    HostCall::SwitchToInputMode(InputMode::Locked),
                ],
                "{mode:?}"
            );
        }
    }

    #[test]
    fn focusing_an_editor_while_scrolling_locks_once_scroll_mode_ends() {
        let mut host = MockHost::default();
        host.focus(PANE_1)
            .running(PANE_1, &["/bin/zsh"])
            .running(PANE_2, &["nvim"]);
        let mut state = started(&[], &mut host);

        // In Scroll mode, focus moves to nvim. Scroll mode is left alone.
        state.handle_event(mode_update(InputMode::Scroll), &mut host);
        host.focus(PANE_2);
        state.handle_event(pane_update(), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_2),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );

        // Back in Normal mode, nvim still owns the pane: lock.
        state.handle_event(mode_update(InputMode::Normal), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_2),
                HostCall::SwitchToInputMode(InputMode::Locked),
            ]
        );
    }

    #[test]
    fn leaving_a_mode_straight_to_locked_is_left_alone() {
        let mut host = MockHost::default();
        host.focus(PANE_1)
            .running(PANE_1, &["nvim"])
            .running(PANE_2, &["/bin/zsh"]);
        let mut state = started(&[], &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);

        // While scrolling, focus moves to a shell, which calls for Normal. The
        // user leaves Scroll mode straight to Locked, and that choice sticks,
        // as does unlocking by hand from there.
        state.handle_event(mode_update(InputMode::Scroll), &mut host);
        host.focus(PANE_2);
        state.handle_event(pane_update(), &mut host);
        settle(&mut state, &mut host);
        host.drain();
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(mode_update(InputMode::Normal), &mut host);
        assert_eq!(host.drain(), vec![]);
    }

    #[test]
    fn manual_mode_change_is_respected_while_command_is_unchanged() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);

        // vim locks; the user unlocks by hand. The next check sees the same
        // command and must not lock again.
        host.running(PANE_1, &["vim"]);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(mode_update(InputMode::Normal), &mut host);
        settle(&mut state, &mut host);
        host.drain();
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
            ]
        );

        // The reverse: back at the shell, the user locks by hand and stays locked.
        host.running(PANE_1, &["zsh"]);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        settle(&mut state, &mut host);
        host.drain();
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
            ]
        );
    }

    #[test]
    fn focus_events_query_the_focused_pane_and_assess_only_a_new_one() {
        let mut host = MockHost::default();
        host.focus(PANE_1)
            .running(PANE_1, &["zsh"])
            .running(PANE_2, &["zsh"]);
        let mut state = started(&[], &mut host);

        // Tab and pane updates fire often. With the same pane focused, each
        // costs one focus query and nothing else.
        state.handle_event(tab_update(), &mut host);
        state.handle_event(pane_update(), &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::FocusedPane, HostCall::FocusedPane]
        );

        // A different pane is assessed once, then left to the timer.
        host.focus(PANE_2);
        state.handle_event(pane_update(), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_2),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
        state.handle_event(tab_update(), &mut host);
        assert_eq!(host.drain(), vec![HostCall::FocusedPane]);
    }

    #[test]
    fn focus_change_re_evaluates_an_unchanged_command() {
        let mut host = MockHost::default();
        host.focus(PANE_1)
            .running(PANE_1, &["zsh"])
            .running(PANE_2, &["vim"]);
        let mut state = started(&[], &mut host);

        // Pane 1 runs vim; the user manually unlocks, which must be respected
        // while the command stays the same.
        host.running(PANE_1, &["vim"]);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(mode_update(InputMode::Normal), &mut host);
        settle(&mut state, &mut host);
        host.drain();

        // Moving to pane 2, which also runs vim, is a fresh assessment: lock.
        host.focus(PANE_2);
        state.handle_event(tab_update(), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_2),
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn returning_to_an_idle_pane_unlocks() {
        let mut host = MockHost::default();
        host.focus(PANE_1)
            .running(PANE_1, &["/bin/zsh"])
            .running(PANE_2, &["nvim"]);
        let mut state = started(&[], &mut host);

        // A new tab runs nvim: lock.
        host.focus(PANE_2);
        state.handle_event(tab_update(), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        settle(&mut state, &mut host);
        host.drain();

        // Back to the first tab, whose shell is idle: unlock.
        host.focus(PANE_1);
        state.handle_event(pane_update(), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SwitchToInputMode(InputMode::Normal),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn plugin_pane_is_assessed_by_its_location() {
        let mut host = MockHost::default();
        host.focus(PANE_1)
            .running(PANE_1, &["zsh"])
            .plugin(PLUGIN_PANE, "zellij:session-manager");
        let mut state = started(&[("lock_regex", "^zellij:")], &mut host);

        host.focus(PLUGIN_PANE);
        state.handle_event(pane_update(), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PluginUrl(PLUGIN_PANE),
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn query_error_while_locked_does_not_unlock() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);
        host.running(PANE_1, &["nvim", "main.rs"]);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        settle(&mut state, &mut host);
        host.drain();

        // A failed query is not "nothing is running": no switch, no follow-up.
        host.failing(PANE_1);
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
            ]
        );

        // Nor did it reset the cache: the editor, seen again, is no change.
        host.running(PANE_1, &["nvim", "main.rs"]);
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
            ]
        );
    }

    #[test]
    fn failed_focus_query_keeps_the_cached_pane() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);

        host.focused = None;
        state.handle_event(pane_update(), &mut host);
        assert_eq!(host.drain(), vec![HostCall::FocusedPane]);

        // Checks go on with the pane last known to be focused.
        host.running(PANE_1, &["vim"]);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn unknown_focus_is_queried_again_on_the_next_event() {
        let mut state = State::default();
        let mut host = MockHost::default();

        // Focus cannot be found at the grant: nothing to assess yet.
        state.handle_event(granted(), &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::HideSelf, HostCall::FocusedPane]
        );

        // A check asks again rather than guessing.
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(host.drain(), vec![HostCall::FocusedPane]);

        // Once Zellij answers, the pane is assessed.
        host.focus(PANE_1).running(PANE_1, &["nvim"]);
        state.handle_event(tab_update(), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }
}

/// `Event::CommandChanged`: Zellij pushing the focused pane's new command.
///
/// The scripted world is kept in step with the events, because the follow-up
/// check an event arms queries it.
mod command_changed {
    use super::*;

    /// A started plugin whose client has pane 1 focused, idle at a `zsh` prompt.
    fn at_prompt(host: &mut MockHost) -> State {
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        started(&[], host)
    }

    #[test]
    fn locks_when_editor_starts_and_unlocks_when_it_exits() {
        let mut host = MockHost::default();
        let mut state = at_prompt(&mut host);

        // The editor takes the foreground: lock, with no query needed.
        host.running(PANE_1, &["nvim", "main.rs"]);
        state.handle_event(
            command_changed(PANE_1, &["nvim", "main.rs"], true),
            &mut host,
        );
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );

        // Zellij confirms the switch; the follow-up check finds the same command.
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::FocusedPane, HostCall::PaneCommand(PANE_1)]
        );

        // The shell is back in the foreground. Zellij reports the shell's own
        // argv, not "no command".
        host.running(PANE_1, &["zsh"]);
        state.handle_event(command_changed(PANE_1, &["zsh"], false), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SwitchToInputMode(InputMode::Normal),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn event_for_the_command_a_check_saw_ends_the_rechecks() {
        let mut host = MockHost::default();
        let mut state = at_prompt(&mut host);

        // The check after `nvim⏎` sees the editor, locks, and keeps looking.
        host.running(PANE_1, &["nvim"]);
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        host.drain();

        // Zellij's ticker reports it too, so it will report the exit itself.
        state.handle_event(command_changed(PANE_1, &["nvim"], true), &mut host);
        assert_eq!(host.drain(), vec![]);

        // The pending check still runs, and is the last.
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::FocusedPane, HostCall::PaneCommand(PANE_1)]
        );
        assert_eq!(state.timers_pending, 0);
    }

    #[test]
    fn not_foreground_does_not_mean_idle() {
        // In a command pane (`zellij run -- nvim`, or a layout `command`) the
        // program is the pane's own process, so Zellij reports it with
        // `is_foreground == false`. It still locks.
        let mut host = MockHost::default();
        let mut state = at_prompt(&mut host);

        host.running(PANE_1, &["nvim", "main.rs"]);
        state.handle_event(
            command_changed(PANE_1, &["nvim", "main.rs"], false),
            &mut host,
        );
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn events_for_unfocused_panes_are_ignored_and_keep_the_focus_cache() {
        let mut host = MockHost::default();
        let mut state = at_prompt(&mut host);

        // An editor starts in another pane, which the event even claims this
        // client has focused. The plugin's own focus query says otherwise.
        state.handle_event(command_changed(PANE_2, &["nvim"], true), &mut host);
        assert_eq!(host.drain(), vec![]);
        assert_eq!(state.focus.pane, Some(PANE_1));

        // The focused pane's own events still count.
        host.running(PANE_1, &["vim"]);
        state.handle_event(command_changed(PANE_1, &["vim"], true), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn ignored_while_disabled_or_without_permission() {
        let mut host = MockHost::default();
        let mut state = at_prompt(&mut host);
        state.handle_pipe(pipe(Some("disable")), &mut host);

        host.running(PANE_1, &["vim"]);
        state.handle_event(command_changed(PANE_1, &["vim"], true), &mut host);
        assert_eq!(host.drain(), vec![]);
        // Not even cached, so enabling later still assesses it.
        assert_eq!(state.focus.command, Some(vec!["zsh".to_string()]));

        // Same once the permission is revoked, although the focused pane is known.
        let mut host = MockHost::default();
        let mut state = at_prompt(&mut host);
        state.handle_event(
            Event::PermissionRequestResult(PermissionStatus::Denied),
            &mut host,
        );
        state.handle_event(command_changed(PANE_1, &["vim"], true), &mut host);
        assert_eq!(host.drain(), vec![]);
    }

    #[test]
    fn event_for_a_change_a_query_already_saw_is_a_noop() {
        let mut host = MockHost::default();
        let mut state = at_prompt(&mut host);

        // The check after `nvim⏎` sees the editor and locks...
        host.running(PANE_1, &["nvim", "main.rs"]);
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        host.drain();

        // ...so Zellij's event for the same change, up to a second later, does nothing.
        state.handle_event(
            command_changed(PANE_1, &["nvim", "main.rs"], true),
            &mut host,
        );
        assert_eq!(host.drain(), vec![]);
    }

    #[test]
    fn repeated_command_does_not_undo_a_manual_mode_change() {
        let mut host = MockHost::default();
        let mut state = at_prompt(&mut host);

        // vim locks through the event; the user unlocks by hand.
        host.running(PANE_1, &["vim"]);
        state.handle_event(command_changed(PANE_1, &["vim"], true), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(mode_update(InputMode::Normal), &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        host.drain();

        // The same command again changes nothing.
        state.handle_event(command_changed(PANE_1, &["vim"], true), &mut host);
        assert_eq!(host.drain(), vec![]);
    }

    #[test]
    fn late_event_for_a_pane_just_left_is_ignored() {
        // Events and focus updates come from different Zellij threads, so an
        // event can arrive after the focus has already moved on.
        let mut host = MockHost::default();
        host.running(PANE_2, &["zsh"]);
        let mut state = at_prompt(&mut host);
        host.running(PANE_1, &["nvim"]);
        state.handle_event(command_changed(PANE_1, &["nvim"], true), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);

        // Focus moves to the idle pane 2: unlock.
        host.focus(PANE_2);
        state.handle_event(pane_update(), &mut host);
        state.handle_event(mode_update(InputMode::Normal), &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        host.drain();

        // The editor in pane 1 exits, and that event only arrives now.
        host.running(PANE_1, &["zsh"]);
        state.handle_event(command_changed(PANE_1, &["zsh"], false), &mut host);
        assert_eq!(host.drain(), vec![]);
        assert_eq!(state.focus.pane, Some(PANE_2));
    }

    #[test]
    fn early_event_for_a_pane_about_to_be_focused_is_left_to_the_focus_update() {
        let mut host = MockHost::default();
        host.running(PANE_2, &["nvim"]);
        let mut state = at_prompt(&mut host);

        // Pane 2's event arrives before the plugin learns focus moved there.
        state.handle_event(command_changed(PANE_2, &["nvim"], true), &mut host);
        assert_eq!(host.drain(), vec![]);

        // The focus update queries pane 2 itself and locks.
        host.focus(PANE_2);
        state.handle_event(tab_update(), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_2),
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }
}

/// `handle_pipe`: enable / disable / toggle and forced checks.
mod pipes {
    use super::*;

    #[test]
    fn empty_pipe_forces_an_immediate_check() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);

        state.handle_pipe(pipe(None), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn broadcast_pipe_is_ignored() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);

        state.handle_pipe(
            PipeMessage {
                is_private: false,
                ..pipe(Some("disable"))
            },
            &mut host,
        );
        assert!(state.is_enabled);
        assert_eq!(host.drain(), vec![]);
    }

    #[test]
    fn unknown_pipe_payload_still_triggers_a_check() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);

        state.handle_pipe(pipe(Some("bogus")), &mut host);
        assert!(state.is_enabled);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn empty_pipe_does_not_undo_a_manual_mode_change() {
        // 0.2 configs bind `Enter` to an empty pipe. Pressing it in an editor
        // the user unlocked by hand must not lock again.
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);
        host.running(PANE_1, &["vim"]);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(mode_update(InputMode::Normal), &mut host);
        settle(&mut state, &mut host);
        host.drain();

        state.handle_pipe(pipe(None), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn disable_stops_all_activity_and_enable_resumes_it() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);

        state.handle_pipe(pipe(Some("disable")), &mut host);
        assert!(!state.is_enabled);
        assert_eq!(host.drain(), vec![]);

        // While disabled, nothing is armed, queried, or switched.
        host.running(PANE_1, &["vim"]);
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(mode_update(InputMode::Normal), &mut host);
        state.handle_event(tab_update(), &mut host);
        state.handle_event(pane_update(), &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        state.handle_pipe(pipe(None), &mut host);
        assert_eq!(host.drain(), vec![]);

        // Enabling looks at the focused pane at once.
        state.handle_pipe(pipe(Some("enable")), &mut host);
        assert!(state.is_enabled);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn enable_reassesses_an_unchanged_command_from_the_current_mode() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);

        // While disabled, the user locks by hand. The mode is still tracked.
        state.handle_pipe(pipe(Some("disable")), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        assert_eq!(state.current_mode, InputMode::Locked);
        assert_eq!(host.drain(), vec![]);

        // The pane still runs the shell it ran before disabling, but enabling
        // starts from scratch: Zellij is Locked at a shell, so unlock.
        state.handle_pipe(pipe(Some("enable")), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SwitchToInputMode(InputMode::Normal),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }

    #[test]
    fn toggle_flips_enabled_state() {
        let mut host = MockHost::default();
        host.focus(PANE_1).running(PANE_1, &["zsh"]);
        let mut state = started(&[], &mut host);

        state.handle_pipe(pipe(Some("toggle")), &mut host);
        assert!(!state.is_enabled);
        assert_eq!(host.drain(), vec![]);

        state.handle_pipe(pipe(Some("toggle")), &mut host);
        assert!(state.is_enabled);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::FocusedPane,
                HostCall::PaneCommand(PANE_1),
                HostCall::SetTimeout(RECHECK_DELAY_SECONDS),
            ]
        );
    }
}
