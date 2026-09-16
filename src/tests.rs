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
/// look at the focused pane (and the follow-up check that arms) already done
/// and the resulting host calls drained. Script the world before calling.
fn started(config: &[(&str, &str)], host: &mut MockHost) -> State {
    let mut state = state_with(config);
    state.handle_event(granted(), host);
    if state.recheck_scheduled {
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), host);
    }
    host.drain();
    state
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
        is_private: false,
    }
}

/// `determine_target_mode` and the rules that feed it.
mod decision {
    use super::*;

    #[test]
    fn default_lock_regex_matches_editors() {
        let state = State::default();
        for cmd in ["vim", "nvim", "nvim ~/notes.md", "vim -u NONE file.txt"] {
            assert_eq!(mode_for(&state, cmd), InputMode::Locked, "{cmd}");
        }
    }

    #[test]
    fn default_config_leaves_shells_and_idle_panes_unlocked() {
        let state = State::default();
        for cmd in ["", "zsh", "/bin/zsh", "bash -l", "ls -la", "cat vim.txt"] {
            assert_eq!(mode_for(&state, cmd), InputMode::Normal, "{cmd:?}");
        }
    }

    #[test]
    fn executable_is_extracted_from_absolute_path() {
        let state = State::default();
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

        // The executable is `nvim.exe`. The default `^(vim|nvim)` has no end
        // anchor, so it locks.
        assert_eq!(
            State::default().determine_target_mode(&argv),
            InputMode::Locked
        );

        // An anchored pattern has to allow for the extension.
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
        let state = state_with(&[("lock_regex", ".*")]);
        // Both default ignore patterns, tested against the full command line.
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
        // The default ignore entry is gone, so `zellij` now locks.
        assert_eq!(mode_for(&state, "zellij"), InputMode::Locked);
    }

    #[test]
    fn invalid_regex_fails_closed() {
        let state = state_with(&[("lock_regex", "(unclosed")]);
        assert_eq!(mode_for(&state, "vim"), InputMode::Normal);

        // An invalid ignore regex must not accidentally suppress locking either.
        let state = state_with(&[("ignore_regex", "[")]);
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
        assert!(!state.print_to_log);
        assert_eq!(pattern(&state.lock_regex), Some("^(vim|nvim)"));
        assert_eq!(
            pattern(&state.ignore_regex),
            Some("^(zellij|atuin history start.*)$")
        );
        assert_eq!(pattern(&state.lock_triggers_deprecated), None);
    }

    #[test]
    fn configuration_overrides_defaults() {
        let state = state_with(&[
            ("is_enabled", "false"),
            ("lock_regex", "^(vim|htop)$"),
            ("ignore_regex", "^zellij$"),
            ("triggers", "less|more"),
            ("print_to_log", "true"),
        ]);
        assert!(!state.is_enabled);
        assert!(state.print_to_log);
        assert_eq!(pattern(&state.lock_regex), Some("^(vim|htop)$"));
        assert_eq!(pattern(&state.ignore_regex), Some("^zellij$"));
        assert_eq!(
            pattern(&state.lock_triggers_deprecated),
            Some("^(less|more)$")
        );
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
        state.handle_pipe(pipe(None), &mut host);

        // Only the pipe's timer is armed; it needs no permission.
        assert_eq!(
            host.drain(),
            vec![HostCall::SetTimeout(RECHECK_DELAY_SECONDS)]
        );
    }

    #[test]
    fn input_schedules_a_single_debounced_timer() {
        let mut state = State::default();
        let mut host = MockHost::default();

        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::InputReceived, &mut host);

        assert_eq!(
            host.drain(),
            vec![HostCall::SetTimeout(RECHECK_DELAY_SECONDS)]
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

        // Zellij confirms the switch; the follow-up finds the same command.
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::FocusedPane, HostCall::PaneCommand(PANE_1)]
        );

        // Editor exits back to the shell: switch to Normal.
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
        }
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
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        host.drain();
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::FocusedPane, HostCall::PaneCommand(PANE_1)]
        );

        // The reverse: back at the shell, the user locks by hand and stays locked.
        host.running(PANE_1, &["zsh"]);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        host.drain();
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::FocusedPane, HostCall::PaneCommand(PANE_1)]
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
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
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
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
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
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        host.drain();

        // A failed query is not "nothing is running": no switch, no follow-up.
        host.failing(PANE_1);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::FocusedPane, HostCall::PaneCommand(PANE_1)]
        );

        // Nor did it reset the cache: the editor, seen again, is no change.
        host.running(PANE_1, &["nvim", "main.rs"]);
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::FocusedPane, HostCall::PaneCommand(PANE_1)]
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
        state.handle_event(Event::Timer(RECHECK_DELAY_SECONDS), &mut host);
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
