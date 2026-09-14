//! Native unit tests for the plugin's event logic.
//!
//! These run on the host target (`just test`), not in wasm. Zellij is replaced
//! by `MockHost`, which records every call the plugin makes so each test can
//! assert exactly which host actions a sequence of events produced. Queries
//! are recorded too, and answered from a small world each test scripts up
//! front (`focus`, `running`, `failing`, `plugin`).
//!
//! Tests are grouped into `decision`, `config`, `events`, and `pipes`, so one
//! group can be run alone with e.g. `just test events::`.

use super::*;
use std::collections::HashMap;

#[derive(Debug, PartialEq)]
#[expect(
    dead_code,
    reason = "query variants are first asserted in PLAN.md step 2"
)]
enum HostCall {
    ListClients,
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
#[expect(dead_code, reason = "first used by the ported tests in PLAN.md step 2")]
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
    fn list_clients(&mut self) {
        self.calls.push(HostCall::ListClients);
    }

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

/// A `ListClients` event where the current client runs `command`.
fn clients_running(command: &str) -> Event {
    Event::ListClients(vec![
        ClientInfo {
            client_id: 2,
            pane_id: PaneId::Terminal(9),
            running_command: "htop".to_string(),
            is_current_client: false,
        },
        current_client(command),
    ])
}

fn mode_update(mode: InputMode) -> Event {
    Event::ModeUpdate(ModeInfo {
        mode,
        ..Default::default()
    })
}

/// A `TabUpdate` event with the tab at `active_position` focused.
///
/// Tabs are created in order and never closed, so each tab's id equals its
/// position. Use `tab_update_with_ids` to model closed or moved tabs.
fn tab_update(active_position: usize) -> Event {
    tab_update_with_ids(&(0..=active_position).collect::<Vec<_>>(), active_position)
}

/// A `TabUpdate` event listing tabs with the given ids, in position order,
/// with the tab at `active_position` focused.
fn tab_update_with_ids(tab_ids: &[usize], active_position: usize) -> Event {
    Event::TabUpdate(
        tab_ids
            .iter()
            .enumerate()
            .map(|(position, &tab_id)| TabInfo {
                position,
                tab_id,
                active: position == active_position,
                ..Default::default()
            })
            .collect(),
    )
}

/// A `ListClients` entry for the current client running `command`.
fn current_client(command: &str) -> ClientInfo {
    ClientInfo {
        client_id: 1,
        pane_id: PaneId::Terminal(1),
        running_command: command.to_string(),
        is_current_client: true,
    }
}

/// A `PaneUpdate` event where terminal pane `focused_id` is focused in tab `tab_position`.
fn pane_update(tab_position: usize, focused_id: u32) -> Event {
    let panes = vec![
        PaneInfo {
            id: focused_id + 100,
            is_focused: false,
            ..Default::default()
        },
        PaneInfo {
            id: focused_id,
            is_focused: true,
            ..Default::default()
        },
    ];
    Event::PaneUpdate(PaneManifest {
        panes: HashMap::from([(tab_position, panes)]),
    })
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

/// `determine_target_mode` and the helpers that feed it.
mod decision {
    use super::*;

    #[test]
    fn default_lock_regex_matches_editors() {
        let state = State::default();
        for cmd in ["vim", "nvim", "nvim ~/notes.md", "vim -u NONE file.txt"] {
            assert_eq!(state.determine_target_mode(cmd), InputMode::Locked, "{cmd}");
        }
    }

    #[test]
    fn default_config_leaves_shells_and_idle_panes_unlocked() {
        let state = State::default();
        for cmd in ["", "zsh", "bash -l", "ls -la", "cat vim.txt"] {
            assert_eq!(
                state.determine_target_mode(cmd),
                InputMode::Normal,
                "{cmd:?}"
            );
        }
    }

    #[test]
    fn executable_is_extracted_from_absolute_path() {
        let state = State::default();
        assert_eq!(
            state.determine_target_mode("/usr/local/bin/nvim --clean"),
            InputMode::Locked
        );
    }

    #[test]
    fn executable_is_extracted_from_parenthesised_process_name() {
        // Some shells report background/child processes as `(name)`.
        let state = state_with(&[("lock_regex", "^atuin$")]);
        assert_eq!(state.determine_target_mode("(atuin)"), InputMode::Locked);
        assert_eq!(
            state.determine_target_mode("/usr/bin/atuin search -i"),
            InputMode::Locked
        );
    }

    #[test]
    fn lock_regex_is_also_tested_against_full_command_line() {
        let state = state_with(&[("lock_regex", "^ssh .*prod")]);
        assert_eq!(
            state.determine_target_mode("ssh deploy@prod-1"),
            InputMode::Locked
        );
        assert_eq!(
            state.determine_target_mode("ssh deploy@staging"),
            InputMode::Normal
        );
    }

    #[test]
    fn ignore_regex_overrides_lock_regex() {
        let state = state_with(&[("lock_regex", ".*")]);
        // Both default ignore patterns, tested against the full command line.
        assert_eq!(state.determine_target_mode("zellij"), InputMode::Normal);
        assert_eq!(
            state.determine_target_mode("atuin history start -- ls"),
            InputMode::Normal
        );
        // Sanity: the catch-all lock regex still locks anything else.
        assert_eq!(state.determine_target_mode("htop"), InputMode::Locked);
    }

    #[test]
    fn custom_ignore_regex_replaces_default() {
        let state = state_with(&[("lock_regex", ".*"), ("ignore_regex", "^htop$")]);
        assert_eq!(state.determine_target_mode("htop"), InputMode::Normal);
        // The default ignore entry is gone, so `zellij` now locks.
        assert_eq!(state.determine_target_mode("zellij"), InputMode::Locked);
    }

    #[test]
    fn invalid_regex_fails_closed() {
        let state = state_with(&[("lock_regex", "(unclosed")]);
        assert_eq!(state.determine_target_mode("vim"), InputMode::Normal);

        // An invalid ignore regex must not accidentally suppress locking either.
        let state = state_with(&[("ignore_regex", "[")]);
        assert_eq!(state.determine_target_mode("vim"), InputMode::Locked);
    }

    #[test]
    fn empty_lock_regex_never_locks() {
        let state = state_with(&[("lock_regex", "")]);
        assert_eq!(pattern(&state.lock_regex), None);
        assert_eq!(state.determine_target_mode("vim"), InputMode::Normal);
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
    fn catch_all_lock_regex_leaves_idle_pane_unlocked() {
        // The README's recommended config. An idle pane reports no command
        // (`running_command` maps "N/A" to ""), and `.*` must not lock on that.
        let state = state_with(&[("lock_regex", ".*")]);
        assert_eq!(state.determine_target_mode(""), InputMode::Normal);
        assert_eq!(state.determine_target_mode("htop"), InputMode::Locked);
    }

    #[test]
    fn readme_recommended_config_locks_everything_but_the_shell() {
        let config = readme_config();
        let config: Vec<(&str, &str)> = config.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let state = state_with(&config);
        assert_eq!(pattern(&state.lock_regex), Some(".*"));

        // Shells, prompt tooling, and an idle pane stay unlocked.
        for cmd in [
            "",
            "zsh",
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
            assert_eq!(
                state.determine_target_mode(cmd),
                InputMode::Normal,
                "{cmd:?}"
            );
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
            assert_eq!(
                state.determine_target_mode(cmd),
                InputMode::Locked,
                "{cmd:?}"
            );
        }
    }

    #[test]
    fn deprecated_triggers_are_anchored_whole_word_alternatives() {
        let state = state_with(&[("lock_regex", ""), ("triggers", "htop|less")]);
        assert_eq!(state.determine_target_mode("htop"), InputMode::Locked);
        assert_eq!(
            state.determine_target_mode("less README.md"),
            InputMode::Locked
        );
        // Anchored: a prefix match is not enough.
        assert_eq!(state.determine_target_mode("htopx"), InputMode::Normal);
    }

    #[test]
    fn running_command_is_trimmed_and_na_means_no_command() {
        assert_eq!(running_command(&current_client("N/A")), "");
        assert_eq!(running_command(&current_client(" N/A ")), "");
        assert_eq!(running_command(&current_client("")), "");
        assert_eq!(
            running_command(&current_client("  vim main.rs ")),
            "vim main.rs"
        );
        // Only the exact sentinel is special.
        assert_eq!(running_command(&current_client("N/A tool")), "N/A tool");
    }
}

/// `load_configuration`: defaults, overrides, and value parsing.
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
        assert_eq!(state.reaction_seconds, 0.3);
    }

    #[test]
    fn configuration_overrides_defaults() {
        let state = state_with(&[
            ("is_enabled", "false"),
            ("lock_regex", "^(vim|htop)$"),
            ("ignore_regex", "^zellij$"),
            ("triggers", "less|more"),
            ("reaction_seconds", "1.5"),
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
        assert_eq!(state.reaction_seconds, 1.5);
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
}

/// `handle_event`: timers, mode switches, and tab/pane tracking.
mod events {
    use super::*;

    #[test]
    fn input_schedules_a_single_debounced_timer() {
        let mut state = State::default();
        let mut host = MockHost::default();

        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(mode_update(InputMode::Normal), &mut host);

        assert_eq!(host.drain(), vec![HostCall::SetTimeout(0.3)]);
    }

    #[test]
    fn timer_uses_configured_reaction_seconds() {
        let mut state = state_with(&[("reaction_seconds", "0.05")]);
        let mut host = MockHost::default();

        state.handle_event(Event::InputReceived, &mut host);

        assert_eq!(host.drain(), vec![HostCall::SetTimeout(0.05)]);
    }

    #[test]
    fn timer_lists_clients_and_allows_rearming() {
        let mut state = State::default();
        let mut host = MockHost::default();

        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(Event::Timer(0.3), &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::SetTimeout(0.3), HostCall::ListClients]
        );

        // The timer fired, so new input may schedule another one.
        state.handle_event(Event::InputReceived, &mut host);
        assert_eq!(host.drain(), vec![HostCall::SetTimeout(0.3)]);
    }

    #[test]
    fn locks_when_editor_starts_and_unlocks_when_it_exits() {
        let mut state = State::default();
        let mut host = MockHost::default();

        // Shell is idle: nothing to do.
        state.handle_event(clients_running("zsh"), &mut host);
        assert_eq!(host.drain(), vec![HostCall::SetTimeout(0.3)]);
        state.handle_event(Event::Timer(0.3), &mut host);
        host.drain();

        // Editor starts: switch to Locked and re-check shortly after.
        state.handle_event(clients_running("nvim main.rs"), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(0.3),
            ]
        );

        // Zellij confirms the switch, timer fires, command unchanged: no-op.
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(Event::Timer(0.3), &mut host);
        host.drain();
        state.handle_event(clients_running("nvim main.rs"), &mut host);
        assert_eq!(host.drain(), vec![]);

        // Editor exits back to the shell: switch to Normal.
        state.handle_event(clients_running("zsh"), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SwitchToInputMode(InputMode::Normal),
                HostCall::SetTimeout(0.3),
            ]
        );
    }

    #[test]
    fn does_not_switch_when_already_in_target_mode() {
        let mut state = State::default();
        let mut host = MockHost::default();

        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(Event::Timer(0.3), &mut host);
        host.drain();

        // The command changed, so a follow-up check is armed, but no switch.
        state.handle_event(clients_running("vim"), &mut host);
        assert_eq!(host.drain(), vec![HostCall::SetTimeout(0.3)]);
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
            let mut state = State::default();
            let mut host = MockHost::default();

            state.handle_event(mode_update(mode), &mut host);
            state.handle_event(Event::Timer(0.3), &mut host);
            host.drain();

            // The command changed, so a follow-up check is armed, but no switch.
            state.handle_event(clients_running("vim"), &mut host);
            assert_eq!(host.drain(), vec![HostCall::SetTimeout(0.3)], "{mode:?}");
        }
    }

    #[test]
    fn na_running_command_is_treated_as_no_command() {
        let mut state = State::default();
        let mut host = MockHost::default();

        // The first report is assessed (already Normal, so no switch). After
        // that, "N/A" and an empty command are the same: not a change.
        state.handle_event(clients_running("N/A"), &mut host);
        assert_eq!(host.drain(), vec![HostCall::SetTimeout(0.3)]);
        state.handle_event(Event::Timer(0.3), &mut host);
        host.drain();
        state.handle_event(clients_running(""), &mut host);
        assert_eq!(host.drain(), vec![]);

        // Leaving an editor for a pane that reports "N/A" still unlocks.
        state.handle_event(clients_running("vim"), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(Event::Timer(0.3), &mut host);
        host.drain();
        state.handle_event(clients_running(" N/A "), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SwitchToInputMode(InputMode::Normal),
                HostCall::SetTimeout(0.3),
            ]
        );
    }

    #[test]
    fn ignores_list_without_a_current_client() {
        let mut state = State::default();
        let mut host = MockHost::default();

        state.handle_event(
            Event::ListClients(vec![ClientInfo {
                client_id: 2,
                pane_id: PaneId::Terminal(9),
                running_command: "vim".to_string(),
                is_current_client: false,
            }]),
            &mut host,
        );
        assert_eq!(host.drain(), vec![]);
    }

    #[test]
    fn manual_mode_change_is_respected_while_command_is_unchanged() {
        let mut state = State::default();
        let mut host = MockHost::default();

        // vim locks; the user unlocks by hand. The next check sees the same
        // command and must not lock again.
        state.handle_event(clients_running("vim"), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(mode_update(InputMode::Normal), &mut host);
        state.handle_event(Event::Timer(0.3), &mut host);
        host.drain();
        state.handle_event(clients_running("vim"), &mut host);
        assert_eq!(host.drain(), vec![]);

        // The reverse: back at the shell, the user locks by hand and stays locked.
        state.handle_event(clients_running("zsh"), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(Event::Timer(0.3), &mut host);
        host.drain();
        state.handle_event(clients_running("zsh"), &mut host);
        assert_eq!(host.drain(), vec![]);
    }

    #[test]
    fn tab_switch_re_evaluates_an_unchanged_command() {
        let mut state = State::default();
        let mut host = MockHost::default();

        // Tab 0 runs vim; the user manually unlocks, which must be respected
        // while the command stays the same.
        state.handle_event(tab_update(0), &mut host);
        state.handle_event(clients_running("vim"), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(mode_update(InputMode::Normal), &mut host);
        state.handle_event(Event::Timer(0.3), &mut host);
        host.drain();
        state.handle_event(clients_running("vim"), &mut host);
        assert_eq!(host.drain(), vec![]);

        // Switching to tab 1, which also runs vim, is a fresh assessment: lock.
        state.handle_event(tab_update(1), &mut host);
        assert_eq!(host.drain(), vec![HostCall::ListClients]);
        state.handle_event(clients_running("vim"), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SwitchToInputMode(InputMode::Locked),
                HostCall::SetTimeout(0.3),
            ]
        );
    }

    #[test]
    fn returning_to_an_idle_tab_unlocks() {
        let mut state = State::default();
        let mut host = MockHost::default();

        // Tab 0 is an idle shell in Normal mode.
        state.handle_event(tab_update(0), &mut host);
        state.handle_event(clients_running("N/A"), &mut host);
        state.handle_event(Event::Timer(0.3), &mut host);
        host.drain();

        // A new tab 1 runs nvim: lock.
        state.handle_event(tab_update(1), &mut host);
        state.handle_event(clients_running("nvim"), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(Event::Timer(0.3), &mut host);
        host.drain();

        // Back to tab 0. Its idle pane reports no command, which must not be
        // mistaken for the "not yet known" state left by the tab change.
        state.handle_event(tab_update_with_ids(&[0, 1], 0), &mut host);
        assert_eq!(host.drain(), vec![HostCall::ListClients]);
        state.handle_event(clients_running("N/A"), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SwitchToInputMode(InputMode::Normal),
                HostCall::SetTimeout(0.3),
            ]
        );
    }

    #[test]
    fn tab_switch_looks_up_clients_immediately() {
        // Zellij emits PaneUpdate before TabUpdate on a tab switch (see
        // zellij-server/src/screen.rs, log_and_report_session_state). Model that order.
        let mut state = State::default();
        let mut host = MockHost::default();

        state.handle_event(tab_update(0), &mut host);
        state.handle_event(pane_update(0, 7), &mut host);
        host.drain();

        // Manifest now carries both tabs; tab 1's pane is focused there, but the
        // plugin still thinks tab 0 is current, so this PaneUpdate is a no-op.
        let manifest = PaneManifest {
            panes: HashMap::from([
                (
                    0,
                    vec![PaneInfo {
                        id: 7,
                        is_focused: true,
                        ..Default::default()
                    }],
                ),
                (
                    1,
                    vec![PaneInfo {
                        id: 8,
                        is_focused: true,
                        ..Default::default()
                    }],
                ),
            ]),
        };
        state.handle_event(Event::PaneUpdate(manifest), &mut host);
        assert_eq!(host.drain(), vec![]);

        // The TabUpdate is what tells the plugin the tab changed: look up immediately.
        state.handle_event(tab_update(1), &mut host);
        assert_eq!(host.drain(), vec![HostCall::ListClients]);

        // Same tab again: nothing.
        state.handle_event(tab_update(1), &mut host);
        assert_eq!(host.drain(), vec![]);
    }

    #[test]
    fn tab_change_is_detected_by_id_not_position() {
        // Tabs 0, 1, 2 exist (ids 0, 1, 2); tab 1 is focused and runs vim.
        let mut state = State::default();
        let mut host = MockHost::default();

        state.handle_event(tab_update_with_ids(&[0, 1, 2], 1), &mut host);
        state.handle_event(clients_running("vim"), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        state.handle_event(Event::Timer(0.3), &mut host);
        host.drain();

        // Closing tab 0 shifts the focused tab to position 0. Same tab, same
        // command: nothing to re-assess.
        state.handle_event(tab_update_with_ids(&[1, 2], 0), &mut host);
        assert_eq!(host.drain(), vec![]);
        state.handle_event(clients_running("vim"), &mut host);
        assert_eq!(host.drain(), vec![]);

        // Closing the vim tab focuses tab 2, which lands at position 0 as well.
        // A different tab at the same position must reset the cached command and
        // look up the new command immediately.
        state.handle_event(tab_update_with_ids(&[2], 0), &mut host);
        assert_eq!(host.drain(), vec![HostCall::ListClients]);
        assert_eq!(state.latest_tab_pane.tab_id, Some(2));
        assert_eq!(state.latest_tab_pane.tab_pos, Some(0));
        state.handle_event(clients_running("zsh"), &mut host);
        assert_eq!(
            host.drain(),
            vec![
                HostCall::SwitchToInputMode(InputMode::Normal),
                HostCall::SetTimeout(0.3),
            ]
        );
    }

    #[test]
    fn pane_focus_change_triggers_immediate_client_lookup() {
        let mut state = State::default();
        let mut host = MockHost::default();

        // Before any `TabUpdate` there is no focused tab to look the pane up in.
        state.handle_event(pane_update(0, 7), &mut host);
        assert_eq!(host.drain(), vec![]);
        assert_eq!(state.latest_tab_pane.pane_id, None);

        state.handle_event(tab_update(0), &mut host);
        host.drain();
        state.handle_event(pane_update(0, 7), &mut host);
        assert_eq!(host.drain(), vec![HostCall::ListClients]);
        assert_eq!(state.latest_tab_pane.pane_id, Some(7));

        // Same focused pane again: nothing new to look up.
        state.handle_event(pane_update(0, 7), &mut host);
        assert_eq!(host.drain(), vec![]);

        // Focus moves to another pane in the same tab.
        state.handle_event(pane_update(0, 8), &mut host);
        assert_eq!(host.drain(), vec![HostCall::ListClients]);

        // Updates for a tab that is not focused are ignored.
        state.handle_event(pane_update(3, 9), &mut host);
        assert_eq!(host.drain(), vec![]);
    }

    #[test]
    fn hides_itself_once_permissions_are_granted() {
        let mut state = State::default();
        let mut host = MockHost::default();

        state.handle_event(
            Event::PermissionRequestResult(PermissionStatus::Denied),
            &mut host,
        );
        assert!(!state.permissions_granted);
        assert_eq!(host.drain(), vec![]);

        state.handle_event(
            Event::PermissionRequestResult(PermissionStatus::Granted),
            &mut host,
        );
        assert!(state.permissions_granted);
        assert_eq!(host.drain(), vec![HostCall::HideSelf]);
    }
}

/// `handle_pipe`: enable / disable / toggle and forced checks.
mod pipes {
    use super::*;

    #[test]
    fn empty_pipe_forces_an_immediate_check() {
        let mut state = State::default();
        let mut host = MockHost::default();

        state.handle_pipe(pipe(None), &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::ListClients, HostCall::SetTimeout(0.3)]
        );
    }

    #[test]
    fn unknown_pipe_payload_still_triggers_a_check() {
        let mut state = State::default();
        let mut host = MockHost::default();

        state.handle_pipe(pipe(Some("bogus")), &mut host);
        assert!(state.is_enabled);
        assert_eq!(
            host.drain(),
            vec![HostCall::ListClients, HostCall::SetTimeout(0.3)]
        );
    }

    #[test]
    fn disable_stops_all_activity_and_enable_resumes_it() {
        let mut state = State::default();
        let mut host = MockHost::default();

        state.handle_pipe(pipe(Some("disable")), &mut host);
        assert!(!state.is_enabled);
        assert_eq!(host.drain(), vec![]);

        // While disabled, no timers are armed and client lists are ignored.
        state.handle_event(Event::InputReceived, &mut host);
        state.handle_event(mode_update(InputMode::Normal), &mut host);
        state.handle_event(clients_running("vim"), &mut host);
        state.handle_pipe(pipe(None), &mut host);
        assert_eq!(host.drain(), vec![]);

        state.handle_pipe(pipe(Some("enable")), &mut host);
        assert!(state.is_enabled);
        assert_eq!(
            host.drain(),
            vec![HostCall::ListClients, HostCall::SetTimeout(0.3)]
        );

        // Back in business.
        state.handle_event(clients_running("vim"), &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::SwitchToInputMode(InputMode::Locked)]
        );
    }

    #[test]
    fn mode_is_tracked_while_disabled_so_enable_starts_from_the_right_mode() {
        let mut state = State::default();
        let mut host = MockHost::default();

        state.handle_pipe(pipe(Some("disable")), &mut host);
        state.handle_event(mode_update(InputMode::Locked), &mut host);
        assert_eq!(state.current_mode, InputMode::Locked);
        assert_eq!(host.drain(), vec![]);

        state.handle_pipe(pipe(Some("enable")), &mut host);
        host.drain();

        // The pane is at a shell but Zellij is Locked: unlock, rather than
        // assuming the mode from before the plugin was disabled.
        state.handle_event(clients_running("zsh"), &mut host);
        assert_eq!(
            host.drain(),
            vec![HostCall::SwitchToInputMode(InputMode::Normal)]
        );
    }

    #[test]
    fn toggle_flips_enabled_state() {
        let mut state = State::default();
        let mut host = MockHost::default();

        state.handle_pipe(pipe(Some("toggle")), &mut host);
        assert!(!state.is_enabled);
        assert_eq!(host.drain(), vec![]);

        state.handle_pipe(pipe(Some("toggle")), &mut host);
        assert!(state.is_enabled);
        assert_eq!(
            host.drain(),
            vec![HostCall::ListClients, HostCall::SetTimeout(0.3)]
        );
    }
}
