# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A headless (no UI) Zellij plugin, compiled to WASM, that automatically switches Zellij between
`Normal` and `Locked` input modes based on which command is running in the focused pane. The
entire plugin is `src/main.rs`; there are no tests.

## Commands

The build target is fixed to `wasm32-wasip1` by `.cargo/config.toml`, so plain `cargo` commands
produce the wasm artifact. `rust-toolchain.toml` pulls in the target and `rustfmt`.

```sh
cargo build --release          # -> target/wasm32-wasip1/release/zellij-autolock.wasm
cargo check                    # fast type-check
cargo fmt                      # formatting (rustfmt is the only formatter used)
just build                     # same as cargo build --release
just install                   # clears Zellij's plugin cache, then builds
```

Zellij caches loaded plugins. After rebuilding, clear the cache or the old wasm keeps running:
`rm -rf ~/.cache/zellij` (Linux) or `~/Library/Caches/org.Zellij-Contributors.Zellij` (macOS).

To test manually, point a Zellij config at the built wasm (`location="file:<path>"`), set
`print_to_log true`, and watch `/tmp/zellij-$(id -u)/zellij-log/zellij.log`. All plugin logging
goes through `eprintln!` guarded by `print_to_log`.

CI lives in `.github/workflows/`: `ci.yaml` runs `cargo fmt --check`, `cargo clippy` (with
`RUSTFLAGS=-Dwarnings`, so any warning fails the job) and a release build on every push and PR,
uploading the wasm as an artifact. `release.yaml` runs on tags matching `v?[0-9]+.*` (existing
tags are unprefixed, e.g. `0.2.2`), builds the wasm, renders release notes from commits with
git-cliff (`cliff.toml`), and creates a **draft** GitHub release with the wasm attached. The
draft must be published manually. `.gitea/workflows/ci.yaml` is a build-and-lint mirror for the
Gitea remote; it does not publish releases. Actions are pinned by commit SHA with the version in
a trailing comment; bump the SHA and comment together. Rust comes from the `stable` channel,
matching `rust-toolchain.toml`.

On this machine `GIT_CLIFF_CONFIG` points at a global config, so always pass
`--config cliff.toml` when previewing release notes locally.

Before pushing, run the same checks locally:

```sh
cargo fmt --all --check
RUSTFLAGS=-Dwarnings cargo clippy --release --all-targets
```

## Architecture (src/main.rs)

`State` is registered via `register_plugin!` and implements `ZellijPlugin` (`load`, `update`,
`pipe`, `render`). `render` is a no-op and `update`/`pipe` always return `false`.

**Detection loop.** The plugin never reads the pane directly; it asks Zellij who the current
client is and what command it is running:

1. Any `InputReceived` or `ModeUpdate` event calls `start_timer()`, which schedules one
   `set_timeout(reaction_seconds)`. `timer_scheduled` debounces so only one timer is pending at a
   time.
2. On `Timer`, the plugin calls `list_clients()`.
3. On `ListClients`, it finds the client with `is_current_client`, reads `running_command`
   (`"N/A"` is normalized to empty), and compares against `latest_tab_pane.command`. Only when
   the command **changed** does it compute a target mode and, if different from `current_mode`,
   call `switch_to_input_mode`. It then re-arms the timer once more to catch fast follow-ups.
4. Mode switches only happen when the current mode is already `Normal` or `Locked`. Other modes
   (Pane, Tab, Scroll, ...) are never overridden.

`TabUpdate` / `PaneUpdate` keep `latest_tab_pane` (tab position + pane id) in sync and reset the
cached command on tab change so a focus change triggers a fresh assessment.

**Mode decision (`determine_target_mode`).** The running command is split into the full command
string and an "exe" (first whitespace token, last path segment, parens stripped so `(atuin)`
becomes `atuin`). Each regex is tested against both forms. Result:
`Locked` if `(lock_regex || triggers) && !ignore_regex`, else `Normal`. Invalid regexes fail
closed (no match) and are logged. Empty pattern or empty text is never a match.

**Config keys** (parsed in `load_configuration`, all strings from the KDL block):
`is_enabled`, `lock_regex`, `ignore_regex`, `reaction_seconds`, `print_to_log`, plus the
deprecated `triggers` (pipe-separated list, wrapped into `^(...)$`; slated for removal after
0.3.0). Booleans accept `true|t|y|1`. `reaction_seconds` is `unwrap()`ed, so a non-numeric value
panics the plugin at load.

**Pipes** (`pipe`): payload `enable` / `disable` / `toggle` flips `is_enabled`; any pipe message
(including no payload) triggers an immediate `list_clients()` + timer when enabled. The README's
keybinding example sends an empty pipe on `Enter` for a snappier reaction than the timer alone.

Permissions requested: `ChangeApplicationState` and `ReadApplicationState`. `hide_self()` is
called once permissions are granted so the plugin pane never shows.

## Docs state

`README.md` is partially stale relative to the code: it still documents `rename_tab` (feature
removed, see `TODO.md`) and refers to the deprecated `triggers` setting. `TODO.md` tracks the
remaining doc work for the 0.3.x line. `CHANGELOG.md` is generated by git-cliff.
