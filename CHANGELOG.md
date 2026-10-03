# Changelog

All notable changes to this project will be documented in this file.

## 0.3.0 - 2026-10-03

**Full Changelog**: https://github.com/fresh2dev/zellij-autolock/compare/0.2.2...0.3.0

### :clap: Features

- *Breaking* - Replace `triggers` with `lock_regex` and `ignore_regex`
- *Breaking* - Require Zellij >= 0.45
- *Breaking* - Default to locking everything except common shells and prompt tooling (0.2 locked only `vim` and `nvim`)
- Replace `print_to_log` with `log_level` (`print_to_log true` still works as `debug` until removed after 0.3.0)
- Detect commands with Zellij's pane queries and `CommandChanged` event instead of polling `list_clients` (thanks to @LittleBear1025-xzh, whose #21 showed the way)
- Apply changes to the plugin's config block without restarting Zellij
- Remove `reaction_seconds`: the recheck delay is fixed at 0.3 seconds, and a config that still sets the key is ignored

### :fist: Fixes

- Fix command detection, which broke with Zellij 0.44 (#18)
- Keep executable paths containing spaces intact, including Windows paths (#19)
- Keep a mode set by hand while the running command is unchanged (#11, #20)
- Lock or unlock on returning to Normal from another mode (Scroll, Pane, Tab, ...) when the focused pane or its command changed in the meantime
- Unlock after a short command that exits before Zellij reports it
- Detect tab changes by tab id instead of position

### Upgrading from 0.2

0.3 replaces the `triggers` option with two regular expressions, `lock_regex` and `ignore_regex`.

Previously, v0.2 only featured `triggers`, and the emphasis was on locking when specific processes launched.

As my list of exceptions continued to grow, I realized it would be better to enter locked mode for *every* process launched inside Zellij, and only unlock the few things that should stay unlocked.

Introducing `lock_regex` and `ignore_regex` in favor of merely `triggers` allows for a more flexible configuration.

- **0.2 was an allowlist.** Zellij stayed in Normal mode unless a specific, named process (`vim`, `fzf`, ...) was launched. Every program that should own the keyboard had to be enumerated in `triggers`.
- **0.3 prefers to lock.** The default is `lock_regex ".*"`, which locks for *every* process launched inside Zellij, combined with an `ignore_regex` that names the few things that should stay unlocked: your shells and prompt tooling. Any interactive program gets the keyboard without being listed.

The old behavior is still available by using `lock_regex` alone as an allowlist, and the two regexes can be combined in any way you like. `ignore_regex` always wins.

| 0.2 | 0.3 |
|-----|-----|
| `triggers "nvim\|vim\|git\|fzf"` | `lock_regex "^(nvim\|vim\|git\|fzf)$"` |
| exact match on the command or executable name | regex match on the full command line **and** the executable name |
| no way to exclude a command | `ignore_regex` overrides `lock_regex` |

`triggers` still works in 0.3.x (it is wrapped as `^(...)$` and OR-ed with `lock_regex`), but it is deprecated and will be removed in the next minor release. Migrate at your convenience.

Other changes worth knowing:

- Requires Zellij >= 0.45 (0.2 required >= 0.41). On older Zellij, stay on 0.2.2.
- Surrounding parentheses are stripped from the executable name, so fish's `(atuin)` is seen as `atuin`.
- The executable name is the last part of the path after `/` or `\`, so `C:\Program Files\Neovim\bin\nvim.exe` is `nvim.exe`.
- The plugin remembers the last command it saw in the focused pane and re-evaluates only when that command, or the focused pane, changes. A mode you set by hand sticks until then.
- With none of `lock_regex`, `ignore_regex`, or `triggers` set, the default changed from locking only `vim` and `nvim` to locking everything except common shells and prompt tooling. Setting any of the three drops both default rules, so a config that sets only `triggers` keeps its allowlist.
- A pane idle at its prompt reports the shell itself (e.g. `/bin/zsh`) instead of no command, so shells have to be in `ignore_regex`. The default lists common ones. To extend it, copy both `lock_regex` and `ignore_regex` from the README's full configuration.
- A focused plugin pane is matched by where it was loaded from, e.g. `zellij:session-manager`.
- The `Enter` keybinding that 0.2 recommended (`WriteChars` plus an empty `MessagePlugin "autolock" {}`) is no longer needed. Zellij locks about 0.3 seconds after `Enter` with or without it, so you can remove it from your config.
- `reaction_seconds` was removed. The delay before the focused pane is checked again is fixed at 0.3 seconds, and the key is ignored if still set. Zellij also reports a changed command on its own, within about a second.
- Log lines renamed: `Trigger commands:` is now `Lock Commands:` and `Ignore Commands:`. New lines: `Focused pane:` and `Command changed in`. Each line is now tagged with its level, e.g. `[autolock] [INFO] ...`. Logging is on by default at `info` (config loads, enable flips, mode switches, and errors), where 0.2 logged nothing unless `print_to_log` was set. Set `log_level "debug"` (what `print_to_log true` now means) to see observed commands.

## 0.2.2 - 2024-12-13

**Full Changelog**: https://github.com/fresh2dev/zellij-autolock/compare/0.2.1...0.2.2

### :clap: Features

- Improve logging

### :fist: Fixes

- Handle absolute paths

### :metal: Other

- bump zellij-tile to 0.41.2

## 0.2.1 - 2024-11-24

**Full Changelog**: https://github.com/fresh2dev/zellij-autolock/compare/0.2.0...0.2.1

### :clap: Features

- Allow autolock to be toggled via pipes

## 0.2.0 - 2024-11-07

**Full Changelog**: https://github.com/fresh2dev/zellij-autolock/compare/0.1.1...0.2.0

### :clap: Features

- *Breaking* - Require Zellij >= 0.41 and rewrite

## 0.1.1 - 2024-10-10

**Full Changelog**: https://github.com/fresh2dev/zellij-autolock/compare/0.1.0...0.1.1

### :fist: Fixes

- Only switch modes from 'normal' or 'locked' modes

### :metal: Other

- DRY

<!-- generated by git-cliff -->
