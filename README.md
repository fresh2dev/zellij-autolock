<h1 align="center">zellij-autolock</h1>
<p align="center"><em>Frictonless Zellij</em></p>
<h2 align="center">
<a href="https://github.com/fresh2dev/zellij-autolock/" target="_blank">Git Repo</a>
</h2>

*zellij-autolock* is a headless [Zellij](https://github.com/zellij-org/zellij) plugin (it has no UI) that checks which command is running in the focused pane and switches Zellij between its **Normal** and **Locked** input modes. When a matching program is in the foreground (Vim, Helix, fzf, ...), Zellij locks and keystrokes go to that program. At the shell prompt, Zellij unlocks and keystrokes go to Zellij. See [Zellij modes](https://zellij.dev/old-documentation/keybindings-modes).

This lets one key do different things depending on the pane: `Ctrl+h` can move focus left at the prompt and still reach Vim inside Vim. See [Keybindings](#keybindings).

> This plugin reacts to user input events, but it does not -- and cannot -- read user input.

```text
+-----------------------------------------------------+
|                  KEYBOARD INPUT                     |
+-------------------------+---------------------------+
                          |
                          V
+-----------------------------------------------------+
|             zellij-autolock PLUGIN                  |
|       (Detects Active Application in Pane)          |
+-------------------------+---------------------------+
                          |
    +--------------------- ----------------------+
    |                                            |
    V [APP DETECTED: Vim, Helix, FZF]            V [NO APP DETECTED]
+----------------------------------+   +----------------------------------+
|      Zellij Mode: LOCKED         |   |      Zellij Mode: NORMAL         |
|   (App Shortcuts Prioritized)    |   |   (Zellij Shortcuts Prioritized) |
+----------------------------------+   +----------------------------------+
          |                                      |
          V                                      V
+----------------------------------+   +----------------------------------+
|     APPLICATION EXECUTION        |   |      ZELLIJ EXECUTION            |
|     (Vim, Helix, FZF, etc.)      |   |   (Pane Mgmt, Layout, etc.)      |
+----------------------------------+   +----------------------------------+
```

## Demo

Navigating Zellij panes running Vim, Neovim, Helix, FZF, and more:

<video autoplay="false" controls="controls" style="width: 800px;">
  <source src="https://img.fresh2.dev/1716528665751_11894996682.webm" type="video/webm"/>
  <p><i>This page does not support webm video playback.</i></p>
  <p><i><a href="https://img.fresh2.dev/1716528665751_11894996682.webm" target="_blank">Click here to watch the demo recording.</a></i></p>
</video>
<p><b><i><a href="https://img.fresh2.dev/1716528665751_11894996682.webm" target="_blank">Open full screen demo recording.</a></i></b></p>

The Zellij mode ("Normal" or "Locked" in the top-right corner) changes with the process running in the focused pane, so the same mappings (`Ctrl+h/j/k/l`) navigate between Zellij panes, Vim windows, FZF results, and more.

## Install

> Requires Zellij >= 0.45.

### 1. Download the plugin

Download `zellij-autolock.wasm` from the [releases page](https://github.com/fresh2dev/zellij-autolock/releases) and save it to your Zellij config path, e.g. `~/.config/zellij/plugins/zellij-autolock.wasm`. You will reference this path in the next step.

Alternatively, if you have [uv](https://docs.astral.sh/uv/) installed, [ghgrab](https://github.com/abhixdd/ghgrab) can fetch the latest release asset in one line:

```sh
uvx ghgrab release fresh2dev/zellij-autolock \
  --asset-regex '.wasm$' \
  --out ~/.config/zellij/plugins/
```

Add `--tag 0.3.0` (or any release tag) to pin a version.

### 2. Register the plugin

Define the plugin in the `plugins` section of your Zellij config and load it at startup. Replace the `location` with the path from step 1 if it differs.

A minimal configuration, which uses the defaults for every option:

```kdl
plugins {
    autolock location="file:~/.config/zellij/plugins/zellij-autolock.wasm"
    // ...
}

load_plugins {
    autolock
    // ...
}
```

A full configuration, with every option set to its default:

```kdl
plugins {
    autolock location="file:~/.config/zellij/plugins/zellij-autolock.wasm" {
        // Start enabled? (default: true)
        is_enabled true
        // Lock when the running command matches this regex.
        // (default: ".*", as shown: lock for everything)
        // Each regex is tested against both the full command line and the
        // bare executable name (e.g. `/usr/bin/nvim foo.txt` and `nvim`).
        lock_regex ".*"
        // ...but never lock when the command matches this regex.
        // (default: common shells and prompt tooling, as shown)
        // A pane idle at its prompt reports the shell itself (e.g. `/bin/zsh`),
        // so shells must be listed here.
        ignore_regex "^(zellij|sh|ash|dash|bash|zsh|fish|ksh|mksh|csh|tcsh|nu|xonsh|elvish|pwsh|powershell|cmd|ls|lsd|eza|starship|direnv|\\(atuin\\)|atuin history start.*)(\\.exe)?$"
        // Zellij log verbosity: trace, debug, info, warn, error, or critical. (default: "info")
        log_level "info"
    }
    // ...
}

load_plugins {
    autolock
    // ...
}
```

The defaults for `lock_regex` and `ignore_regex` only apply together. Setting either of them, or the deprecated `triggers`, drops both defaults, so set every rule you want: `lock_regex ".*"` alone also locks idle shells, and `ignore_regex` alone never locks anything. To add to the default shell list, copy both lines from the full configuration and edit `ignore_regex`.

Two common setups:

- **Lock everything except the shell (the default):** `lock_regex ".*"` plus an `ignore_regex` listing shells and prompt tooling, as in the full configuration above.
- **Allowlist:** `lock_regex "^(vim|nvim|hx|fzf)$"` locks only for the listed programs. This is the 0.2 behavior.

What the regexes are tested against:

- **The foreground program of the focused pane**, with its arguments. Wrappers count as the program: `sudo vim /etc/hosts` has the executable `sudo`, so match the full command line or use the catch-all setup.
- **An idle pane reports its shell**, such as `/bin/zsh`, not "no command". The default `ignore_regex` covers common shells; if yours is missing, add it to that pattern (and keep `lock_regex`).
- **A focused plugin pane reports where it was loaded from**, such as `zellij:session-manager`. The default `ignore_regex` does not match that, so plugin panes lock too. Add `zellij:.*` to `ignore_regex` to keep them unlocked.
- **The executable is the last part of the path**, after `/` or `\`, so `C:\Program Files\Neovim\bin\nvim.exe` is `nvim.exe`.

If a regex fails to compile it never matches, and the error is written to the Zellij log at `error` level, which is on by default.

Changes to this block take effect when Zellij reloads its config, without restarting the session.

## Upgrading from 0.2

| 0.2 | 0.3 |
|-----|-----|
| Zellij >= 0.41 | Zellij >= 0.45 (on older Zellij, stay on 0.2.2) |
| `triggers "nvim\|vim\|fzf"` | `lock_regex "^(nvim\|vim\|fzf)$"` |
| exact match on the command or executable name | regex match on the full command line and the executable name |
| no way to exclude a command | `ignore_regex` overrides `lock_regex` |
| with no `triggers` set, locks only `vim` and `nvim` | with no rules set, locks everything except common shells and prompt tooling |
| `print_to_log true` | `log_level "debug"` |
| `reaction_seconds "0.3"` | removed; the delay is fixed at 0.3 seconds and the key is ignored |
| `Enter` bound to `WriteChars` plus `MessagePlugin "autolock" {}` | not needed; remove it |

`triggers` and `print_to_log` still work in 0.3.x but are deprecated and will be removed after 0.3.0. See the [CHANGELOG](CHANGELOG.md#030---unreleased) for the full list of changes.

## Keybindings

Zellij's default keybindings are modal: `Ctrl+p` enters Pane mode, `h` moves focus left, and `Esc` returns to Normal. Locked mode passes every key to the pane, but is toggled by hand with `Ctrl+g`.

With autolock switching to Locked whenever a matching program is in the foreground, you can:

- Start from `keybinds clear-defaults=true` and put single-key bindings in `shared_except "locked"`. They apply at the shell prompt and pass through to Vim, fzf, etc.
- Put Zellij actions that must work while locked in `shared { ... }`, on a modifier the programs in your panes don't use (the example below uses `Alt`). Keep `locked { ... }` for the keys that leave Locked mode.
- Bind keys to flip the lock by hand (`Alt .`) and to disable autolock and flip the lock (`Alt Shift .`) for programs the regex does not cover.

| Action | Default Zellij | With autolock |
|--------|----------------|---------------|
| Move focus left | `Ctrl+p`, `h` | `Ctrl+h` |
| Scroll half page up | `Ctrl+s`, `u` | `Ctrl+u` |
| Give the keyboard to Vim | `Ctrl+g` (by hand) | automatic |
| Take the keyboard back | `Ctrl+g` (by hand) | automatic, or `Alt+.` |

See [Keybinding comparison](#keybinding-comparison) for the full table.

Excerpt from a working config:

```kdl
keybinds clear-defaults=true {
    // Active only in Locked mode.
    locked {
        bind "Alt ." { SwitchToMode "normal"; }
        bind "Alt Shift ." {
            MessagePlugin "autolock" { payload "toggle"; }
            SwitchToMode "normal"
        }
    }
    // Active in every mode, including Locked.
    shared {
        bind "Alt h" { MoveFocus "left"; }
        bind "Alt j" { MoveFocus "down"; }
        bind "Alt k" { MoveFocus "up"; }
        bind "Alt l" { MoveFocus "right"; }
        bind "Alt n" { NewPane; }
        bind "Alt w" { CloseFocus; }
        bind "Alt t" { NewTab; }
        bind "Alt 1" { GoToTab 1; SwitchToMode "normal"; }
        //...
    }
    // Active in every mode except Locked. Put keys here that Vim, fzf, etc. also use.
    shared_except "locked" {
        bind "Alt ." { SwitchToMode "locked"; }
        bind "Alt Shift ." {
            MessagePlugin "autolock" { payload "toggle"; }
            SwitchToMode "locked"
        }
        bind "Ctrl h" { MoveFocus "left"; }
        bind "Ctrl j" { MoveFocus "down"; }
        bind "Ctrl k" { MoveFocus "up"; }
        bind "Ctrl l" { MoveFocus "right"; }
        bind "Ctrl b" { PageScrollUp; }
        bind "Ctrl f" { PageScrollDown; }
        bind "Ctrl u" { HalfPageScrollUp; }
        bind "Ctrl d" { HalfPageScrollDown; }
    }
    // clear-defaults=true also removes the keys that return to Normal mode.
    shared_except "normal" "locked" "entersearch" {
        bind "enter" { SwitchToMode "normal"; }
    }
    shared_except "normal" "locked" "scroll" "search" "renametab" "renamepane" "session" {
        bind "esc" { SwitchToMode "normal"; }
    }
    //...
}
```

## Keybinding comparison

Zellij's default keybindings (`zellij setup --dump-config`) compared with a sample autolock configuration:

- Most default actions are two-step sequences through a mode (`Ctrl+p` for Pane, `Ctrl+t` for Tab, `Ctrl+s` for Scroll, ...). The sample binds each action to one chord and drops the Pane, Tab, Move, Session, and Tmux modes.
- Bindings in `shared { ... }` (the `Alt` keys) are active in every mode, including Locked. Bindings in `shared_except "locked"` apply only when unlocked, so the sample can reuse keys that Vim, fzf, and less also use, such as `Ctrl+h` (Move mode by default), `Ctrl+b` (Tmux mode), `Ctrl+d`, `Ctrl+f`, and `Ctrl+u`.

| Action | Default Zellij | Sample autolock config | Works while locked |
|--------|----------------|------------------------|--------------------|
| Lock Zellij (send every key to the pane) | `Ctrl+g` | `Alt+.` (or automatic) | — |
| Unlock Zellij | `Ctrl+g` | `Alt+.` (or automatic) | yes |
| Toggle autolock and flip the lock | — | `Alt+Shift+.` | yes |
| Move focus left / down / up / right | `Ctrl+p`, `h` / `j` / `k` / `l` | `Ctrl+h` / `j` / `k` / `l` | no |
| Move focus left / down / up / right (from anywhere) | `Alt+h` / `j` / `k` / `l` | `Alt+h` / `j` / `k` / `l` | yes |
| Scroll half page up / down | `Ctrl+s`, `u` / `d` | `Ctrl+u` / `Ctrl+d` | no |
| Scroll full page up / down | `Ctrl+s`, `Ctrl+b` / `Ctrl+f` | `Ctrl+b` / `Ctrl+f` | no |
| Search scrollback | `Ctrl+s`, `s` | `Alt+/` | yes |
| Edit scrollback in `$EDITOR` | `Ctrl+s`, `e` | `Alt+e` | yes |
| New pane (auto placement) | `Alt+n` | `Alt+n` | yes |
| New pane below | `Ctrl+p`, `d` | ``Alt+` `` | yes |
| New pane to the right | `Ctrl+p`, `r` | `Alt+v` | yes |
| New stacked pane | `Ctrl+p`, `s` | `Alt+Shift+n` | yes |
| Close pane | `Ctrl+p`, `x` | `Alt+w` | yes |
| Toggle fullscreen pane | `Ctrl+p`, `f` | `Alt+m` | yes |
| Toggle host fullscreen (nested sessions) | `Ctrl+o`, `f` | `Alt+Shift+m` | yes |
| Move pane left / down / up / right | `Ctrl+h`, `h` / `j` / `k` / `l` | `Alt+Shift+h` / `j` / `k` / `l` | yes |
| Show / hide floating panes | `Alt+f` | `Alt+Space` | yes |
| Embed or float the focused pane | `Ctrl+p`, `e` | `Alt+f` | yes |
| Pin floating pane | `Ctrl+p`, `i` | `Alt+Enter` | yes |
| Enter Resize mode | `Ctrl+n` | `Alt+Shift+=` or `Alt+Shift+-` | yes |
| Grow / shrink pane | `Alt+=` / `Alt+-` | `Alt+=` / `Alt+-` | yes |
| New tab | `Ctrl+t`, `n` | `Alt+t` | yes |
| Close tab | `Ctrl+t`, `x` | `Alt+Shift+w` | yes |
| Go to tab 1–9 | `Ctrl+t`, `1`–`9` | `Alt+1`–`Alt+9` | yes |
| Previous / next tab | `Ctrl+t`, `h` / `l` | `Alt+Shift+[` / `Alt+Shift+]` | yes |
| Move tab left / right | `Alt+i` / `Alt+o` | `Alt+[` / `Alt+]` | yes |
| Break pane into a new tab | `Ctrl+t`, `b` | `Alt+b` | yes |
| Break pane into the tab on the left | `Ctrl+t`, `[` | `Alt+Shift+b` | yes |
| Next / previous swap layout | `Alt+]` / `Alt+[` | `Alt+o` / `Alt+Shift+o` | yes |
| Open the session manager | `Ctrl+o`, `w` | `Alt+s` | yes |
| Detach | `Ctrl+o`, `d` | `Alt+q` | yes |
| Find a tab by name ([room](https://github.com/rvcas/room)) | — | `Alt+0` | yes |
| Command palette | — | `Alt+Shift+p` | yes |
| Select and copy text from the pane (zextract) | — | `Alt+r` | yes |
| Favorite sessions (zellij-favs) | — | `Alt+Shift+s` | yes |

zextract and zellij-favs are listed in [awesome-zellij](https://github.com/zellij-org/awesome-zellij).

In Scroll and Search mode, the sample keeps Zellij's default keys (`h`/`j`/`k`/`l`, `d`/`u`, `[`/`]` to jump between prompts, `m`, `c`, `e`, `s`, `n`/`p`) and adds `/` to start a search and `i` or `Esc` to return to Normal mode.

Because the sample starts from `clear-defaults=true`, anything not listed above has no keybinding. That includes `Quit` (`Ctrl+q` by default), renaming panes and tabs, syncing a tab, pane groups, toggling pane frames, and the configuration, plugin-manager, share, layout-manager, and about plugins. Bind them yourself or reach them with `zellij action ...` from the CLI.

## Pipes

The plugin accepts Zellij pipe messages so you can drive it from keybindings or the command line.

From a keybinding:

```kdl
// Trigger an immediate assessment of the current pane.
MessagePlugin "autolock" {};
// Disable, enable, or toggle the autolock mechanism.
MessagePlugin "autolock" { payload "disable"; };
MessagePlugin "autolock" { payload "enable"; };
MessagePlugin "autolock" { payload "toggle"; };
```

From the `zellij` CLI:

```sh
zellij pipe --plugin autolock                 # assess the current pane now
zellij pipe --plugin autolock -- "disable"
zellij pipe --plugin autolock -- "enable"
zellij pipe --plugin autolock -- "toggle"
```

Disabling the plugin stops it from switching modes; it does not change the current mode. Combine with `SwitchToMode` in a keybinding if you want both, as in the `Alt Shift .` example above.

## Use with Vim

Install the companion Vim plugin [***zellij.vim***](https://github.com/fresh2dev/zellij.vim) to navigate between Zellij panes and Vim windows with the same keys.

## Troubleshooting

**View [Zellij logs](https://zellij.dev/documentation/plugin-api-logging):**

on Linux...

```sh
tail -f /tmp/zellij-$(id -u)/zellij-log/zellij.log
```

on MacOS...

```sh
find /var/folders -type f -name 'zellij.log' -exec tail -f {} \; 2>/dev/null
```

**Clear the Zellij cache:**

Zellij caches plugins. After replacing the `.wasm` file, clear the cache or the old version keeps running.

on Linux...

```sh
rm -rf ~/.cache/zellij
```

on MacOS...

```sh
rm -rf ~/Library/Caches/org.Zellij-Contributors.Zellij
```

## Shoutouts

- [zellij-org/zellij](https://github.com/zellij-org/zellij) ~ This is what it's all for.
- [dj95/zjstatus](https://github.com/dj95/zjstatus) ~ This is an awesome and complex Zellij plugin I reference often.
- [hiasr/vim-zellij-navigator](https://github.com/hiasr/vim-zellij-navigator) ~ I initially referenced this project when learning how to build a Zellij plugin and react to events.
- [christoomey/vim-tmux-navigator](https://github.com/christoomey/vim-tmux-navigator) ~ This project implements the functionality in tmux I wanted to bring to Zellij
