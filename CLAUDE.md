# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A headless (no UI) Zellij plugin, compiled to WASM, that automatically switches Zellij between
`Normal` and `Locked` input modes based on which command is running in the focused pane. The
plugin is `src/main.rs`; native unit tests live in `src/tests.rs`.

## Commands

The build target is fixed to `wasm32-wasip1` by `.cargo/config.toml`, so plain `cargo` commands
produce the wasm artifact. `rust-toolchain.toml` pulls in the target and `rustfmt`.

```sh
cargo build --release          # -> target/wasm32-wasip1/release/zellij-autolock.wasm
cargo check                    # fast type-check
cargo fmt                      # formatting (rustfmt is the only formatter used)
just build                     # same as cargo build (pass --release for the wasm artifact)
just bootstrap                 # rustup toolchain/target install + cargo fetch (idempotent, slow)
just install                   # bootstrap, clear Zellij's plugin cache, release build, copy wasm
just test                      # cargo test on the host target (see below)
```

**Tests run natively, not in wasm.** Because the build target is pinned to wasm and no wasm
runner is installed, `cargo test` must be given the host triple:
`cargo test --target "$(rustc -vV | sed -n 's/^host: //p')"`, which is what `just test` does.
This works because `zellij-tile` stubs its host import on non-wasm targets. Tests never call
the real shim functions: all calls into Zellij go through the `Host` trait in `src/main.rs`,
and `src/tests.rs` drives `State::handle_event` / `handle_pipe` with `MockHost`, which records
every host call (queries included) and answers queries from a small scripted world
(`focus`, `running`, `failing`, `plugin`). Tests assert the exact sequence of host calls.
`register_plugin!` is gated with `#[cfg(not(test))]` so the test binary has no wasm entry
points. When adding a host call, add it to the `Host` trait, `ZellijHost`, and `MockHost`.

Zellij caches loaded plugins. After rebuilding, clear the cache or the old wasm keeps running:
`rm -rf ~/.cache/zellij` (Linux) or `~/Library/Caches/org.Zellij-Contributors.Zellij` (macOS).

To test manually, point a Zellij config at the built wasm (`location="file:<path>"`), set
`log_level "debug"` (or `"trace"` for every event, timer, and pipe), and watch
`/tmp/zellij-$(id -u)/zellij-log/zellij.log`. All plugin logging goes through
`State::log(level, ..)`, which writes `[autolock] [<LEVEL>] ...` via `eprintln!` when the level
is at or above `log_level`.

For an isolated session that leaves the real config, cache, and logs alone, point `HOME`,
`XDG_CACHE_HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, and `TMPDIR` at a scratch directory, set
`ZELLIJ_CONFIG_FILE`, and run Zellij inside tmux, driving it with `tmux send-keys` and
`zellij -s <session> action ...`. Keep `ZELLIJ_SOCKET_DIR` short (Unix sockets are limited to
about 108 characters). Pre-grant permissions in `$XDG_CACHE_HOME/zellij/permissions.kdl`; the
node name is the bare wasm path, without `file:`. The compact layout shows `NORMAL` / `LOCKED`
for `tmux capture-pane`. Killing tmux leaves the Zellij server running, so kill the session too.

CI lives in `.github/workflows/`: `ci.yaml` runs `cargo fmt --check`, `cargo clippy` (with
`RUSTFLAGS=-Dwarnings`, so any warning fails the job; `--all-targets` includes the test
module, so tests must be warning-free too), `cargo test` on the host target, and a release
build on every push and PR, uploading the wasm as an artifact. The clippy, test, and build
jobs share dependency builds through `Swatinem/rust-cache`. `release.yaml` runs on tags
matching `v?[0-9]+.*` (existing tags are unprefixed, e.g. `0.2.2`), builds the wasm, takes
that version's `## <version> - <date>` section of `CHANGELOG.md` as the release notes, and
creates a **draft** GitHub release with the wasm attached. The job fails if the section is
missing or still says `Unreleased`. The draft must be published manually. Actions are pinned
by commit SHA with the version in a trailing comment; bump the SHA and comment together. Rust
comes from the `stable` channel, matching `rust-toolchain.toml`.

`CHANGELOG.md` is maintained by hand, not in CI. Before tagging a release, draft the new
section locally with git-cliff and the repo's `.cliff.toml` (`just changelog` runs it with
`--unreleased --prepend CHANGELOG.md`; for a draft to compare, run
`uvx --from 'git-cliff==2.*' git-cliff --config .cliff.toml <prev-tag>..HEAD --tag <version>`).
The config skips `docs`, `build`, `ci`, and `test` commits and files `refactor` and `chore`
under "Other", so review the draft rather than pasting it. Add any hand-written notes, put the
release date in the heading, and commit.

Before pushing, run the same checks locally:

```sh
cargo fmt --all --check
RUSTFLAGS=-Dwarnings cargo clippy --all-targets
just test
```

## Architecture (src/main.rs)

`State` is registered via `register_plugin!` and implements `ZellijPlugin` (`load`, `update`,
`pipe`, `render`). `update` and `pipe` are thin wrappers that call `handle_event` /
`handle_pipe` with the real `ZellijHost`; the logic lives in those methods and takes a
`&mut impl Host` so tests can substitute a mock. `render` is a no-op and `update`/`pipe`
always return `false`.

**Detection.** The plugin never reads pane contents. It asks Zellij which pane its client has
focused and what that pane runs, and caches both in `State::focus` (`pane`, and `command`: the
argv last assessed for it; `None` until assessed).

- `TabUpdate` / `PaneUpdate` call `refresh_focus()`: query `get_focused_pane_info`; a different
  pane resets the cached command, and a pane not yet assessed is assessed. The payloads are not
  used: `PaneUpdate` arrives before `TabUpdate` on a tab switch, and a manifest's `is_focused`
  means focused by *any* client.
- Timers: `set_timeout` cannot be cancelled, so `State::timers_pending` counts timers set and
  not yet fired, and a `Timer` that fires while others are pending is stale and ignored.
  `restart_recheck()` always sets a new `set_timeout(RECHECK_DELAY_SECONDS)` (0.3 s constant),
  superseding any pending one; `schedule_recheck()` sets one only if none is pending. A live
  `Timer` uses up one of `rechecks_left`, calls `recheck()` (focus query, then command query),
  and schedules the next check while rounds remain.
- `InputReceived` calls `restart_recheck()`, so the check runs 0.3 s after the *last* key (after
  `Enter`, not the first letter typed). `ModeUpdate` records `current_mode`; returning to `Normal`
  from any mode other than `Locked` clears the cached command and calls `recheck()`, since a
  focus or command change during e.g. Scroll mode was cached without switching. Going straight
  to Locked is the user's choice and is left alone.
- `CommandChanged(pane, argv, is_foreground, _)` calls `assess(argv)` only when `pane` is the
  cached focused pane, and never moves the focus cache (a late event for a pane just left must
  not pull focus back). Zellij emits it from a 1 s ticker, only for panes that printed output
  since the last tick, broadcast to every client's instance. `is_foreground` is only logged: it
  is false both for a shell back at its prompt and for a command pane running its program.
  Afterwards `rechecks_left` is capped at 1: the ticker has seen this command and will report
  the next change, and one check still corrects an event older than the last query.
- `PermissionRequestResult(Granted)` calls `hide_self()` and `refresh_focus()`. Zellij re-sends
  a remembered grant on every load, so this is also the first look after a restart.
- `assess_focused_pane()` asks `get_pane_running_command` for a terminal pane (the foreground
  process via `tcgetpgrp`, else the pane's own process, so an idle shell reports itself) or
  `get_pane_info(..).plugin_url` for a plugin pane. A failed query changes nothing.
- `assess(argv)` acts only when the argv differs from the cached one: it computes the target
  mode, switches if `current_mode` is `Normal` or `Locked` and differs (other modes are never
  overridden; see `ModeUpdate`), and sets `rechecks_left = RECHECK_ROUNDS` (5, about 1.5 s: one
  ticker period plus margin) with `schedule_recheck()`. A command that runs under about a second and prints nothing can exit
  before the ticker sees it, so no event reports the exit and only these checks unlock. An
  unchanged command is left alone, so a mode set by hand sticks.

Zellij API gotchas behind this design:

- The synchronous queries need `ReadApplicationState`. Zellij sends no reply to a denied query
  and the shim panics waiting, so every query is gated on `is_active()` (granted and enabled).
- `get_focused_pane_info` returns the focused tab's *id*, not its position.
- Background plugins (loaded via `load_plugins`) always get `ModeInfo.shell == None`, so the
  plugin cannot recognise an idle default shell. Shells belong in `ignore_regex`, which is why
  `DEFAULT_IGNORE_REGEX` lists common shells (with an optional `.exe`) and prompt tooling.
- `set_timeout` builds a `Duration` from the value inside a detached task, which panics on a
  negative, NaN, or infinite number and silently stops the timer. The delay is a constant for
  that reason, among others.
- CLI `zellij pipe --plugin autolock -- <payload>` delivers the message and exits 0 at once on
  Zellij 0.45.1, from a terminal or a script (sandbox, 2026-10-03). Zellij still logs a spurious
  "Action CliPipe did not complete within 1s timeout" error for each call, about 30 ms after it;
  ignore it. Older notes said the command never exited; that no longer reproduces.
- In the README's keybindings, `Alt Shift .` only fires in terminals that speak the kitty
  keyboard protocol (Ghostty, kitty, WezTerm, foot). Legacy terminals and tmux send `Alt >`, which
  that binding does not match. `tmux send-keys -H 1b 5b 34 36 3b 34 75` (`CSI 46;4u`) sends it
  in the sandbox.

**Mode decision (`determine_target_mode`).** Takes the argv. Each regex is tested against the
joined command line and the executable: the first argument (taken whole, so paths with spaces
survive), its last segment after `/` or `\`, with parens stripped so `(atuin)` becomes `atuin`.
Result: `Locked` if `(lock_regex || triggers) && !ignore_regex`, else `Normal`. Invalid
regexes fail closed (no match) and are logged. Empty pattern or empty text is never a match.

**Config keys** (parsed in `load_configuration`, all strings from the KDL block):
`is_enabled`, `lock_regex` (default `DEFAULT_LOCK_REGEX`, `.*`), `ignore_regex` (default
`DEFAULT_IGNORE_REGEX`), `log_level` (`trace|debug|info|warn|error|critical`, case-insensitive,
default `info`; an unrecognised value keeps `info` and logs a `warn`), plus the deprecated
`triggers` (pipe-separated list, wrapped into `^(...)$`) and `print_to_log` (`true` means
`log_level "debug"`; `log_level` wins if both are set), both slated for removal after 0.3.0.
Levels: `error` for failed queries and invalid regexes, `warn` for bad config values and pipe
payloads, `info` for config loads, enable flips, and mode switches, `debug` for focus changes,
assessed commands, and `CommandChanged`, `trace` for every event, timer, and pipe; `critical`
is unused. The rule defaults are all or nothing:
a block that sets any of `lock_regex`, `ignore_regex`, or `triggers` (even to `""`) starts with
all three unset, so a 0.2 `triggers` allowlist doesn't become "lock everything". Booleans accept
`true|t|y|1`. `reaction_seconds` (0.2.x) was removed; a config that still sets it is silently
ignored, like any unknown key. `PluginConfigurationChanged` goes through
`apply_configuration_change`: Zellij compares against the block the plugin was *loaded* with,
so after one change it resends the same block on every config reload. An identical block is
ignored; otherwise all settings reset to defaults (removed keys revert), the block is loaded, a
pipe-toggled `is_enabled` is kept unless the block edits that key, and the pane is re-assessed.
`InitialKeybinds` is subscribed only so `ModeUpdate` arrives without the keybinding table.

**Pipes** (`pipe`): payload `enable` / `disable` / `toggle` flips `is_enabled`; any pipe message
(including no payload) triggers `recheck()` + timer when enabled. Only a disabled-to-enabled
transition resets the focus cache, so an empty pipe cannot undo a mode set by hand. 0.2's README
bound `Enter` to `WriteChars "\r"` plus an empty pipe for faster locking; 0.3 no longer
recommends it. Its immediate check runs before the shell has started the program, and the key's
own input restarts the timer, so sandbox measurements (2026-09-16) put locking and unlocking at
about 0.32 s with or without it. Old configs that keep it still work.

Permissions requested: `ChangeApplicationState` and `ReadApplicationState`. `hide_self()` is
called once permissions are granted so the plugin pane never shows.

## Docs state

`README.md` matches the code: it documents `lock_regex` / `ignore_regex`, what the rules are
tested against (foreground program, idle shells, plugin pane locations, Windows paths), runtime
config reloads, and lists `triggers` as deprecated. `CHANGELOG.md` dates `0.3.0` 2026-10-03;
the section covers the detection rewrite (crediting PR #21), the new lock-everything default,
and fixes #11, #18, #19, and #20. Commit messages on `dev` carry `Fixes #N` for those four, so
they close when merged to `main`. Once the `0.3.0` tag ships, removing the deprecated `triggers`
and `print_to_log` paths (code, tests, README, and the notes here) is unblocked for the next
release. `src/tests.rs` reads the README's example `lock_regex` / `ignore_regex`
via `include_str!` and asserts they equal the defaults, so change both together.
