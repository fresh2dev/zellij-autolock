# zellij-autolock

*Single-key shortcuts that go to Zellij at the prompt and to your editor everywhere else.*

[Zellij](https://github.com/zellij-org/zellij) is a modern terminal multiplexer, similar to the reverent [tmux](https://github.com/tmux/tmux), but simpler, with a more gradual learning curve.

*zellij-autolock* is a headless Zellij plugin (it has no UI) that watches which command is running in the focused pane and automatically switches Zellij between its **Normal** and **Locked** input modes. When a program you care about is in the foreground (Vim, Helix, fzf, ...), Zellij locks itself and every keystroke reaches that program. When you are back at the shell prompt, Zellij unlocks and your shortcuts drive Zellij again. Read more about Zellij modes [*here*](https://zellij.dev/old-documentation/keybindings-modes).

This removes the need for multi-key, modal keybindings. You can bind `Ctrl+h` to "move focus left" and it will do that at the prompt, while inside Vim the same `Ctrl+h` reaches Vim untouched. See [A different approach to keybindings](#a-different-approach-to-keybindings) for the full picture.

> This plugin reacts to user input events, but it does not -- and cannot -- read user input.

```
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

This plugin offers a different approach to keybindings within Zellij. (TODO: elaborate on this)

## Demo

Here is a demonstration of how you can seamlessly navigate through Zellij panes running Vim, Neovim, Helix, FZF, and more.

<video autoplay="false" controls="controls" style="width: 800px;">
  <source src="https://img.fresh2.dev/1716528665751_11894996682.webm" type="video/webm"/>
  <p><i>This page does not support webm video playback.</i></p>
  <p><i><a href="https://img.fresh2.dev/1716528665751_11894996682.webm" target="_blank">Click here to watch the demo recording.</a></i></p>
</video>
<p><b><i><a href="https://img.fresh2.dev/1716528665751_11894996682.webm" target="_blank">Open full screen demo recording.</a></i></b></p>

Notice how the Zellij mode ( "Normal" or "Locked" in the top-right corner ) automatically toggles depending which process is running within the focused Zellij pane. This allows you to use the same mappings ( `Ctrl+h/j/k/l` ) to navigate between Zellij panes, Vim windows, FZF results, and more.

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

```kdl
plugins {
    autolock location="file:~/.config/zellij/plugins/zellij-autolock.wasm" {
        // Start enabled? (default: true)
        is_enabled true
        // Lock when the running command matches this regex. (default: "^(vim|nvim)")
        // Each regex is tested against both the full command line and the
        // bare executable name (e.g. `/usr/bin/nvim foo.txt` and `nvim`).
        lock_regex ".*"
        // ...but never lock when the command matches this regex.
        // (default: "^(zellij|atuin history start.*)$")
        ignore_regex "^(zellij|sh|zsh|bash|fish|pwsh|ls|eza|starship|direnv|\\(atuin\\)|atuin history start.*)$"
        // React to input after this many seconds. (default: "0.3")
        // (An already-scheduled reaction prevents additional reactions.)
        reaction_seconds "0.3"
        // Print observed commands to the Zellij log for debugging? (default: false)
        print_to_log false
    }
    // ...
}

load_plugins {
    autolock
    // ...
}
```

Two ways to think about `lock_regex`:

- **Allowlist:** `lock_regex "^(vim|nvim|hx|fzf)$"` locks only for programs you name. This is the 0.2 behavior.
- **Lock everything except the shell (recommended):** `lock_regex ".*"` plus an `ignore_regex` listing your shells and prompt tooling, as in the example above. Any interactive program gets the keyboard without you having to enumerate them.

If a regex fails to compile it never matches, and the error is written to the Zellij log when `print_to_log` is on.

## Upgrading from 0.2

0.3 replaces the `triggers` option with `lock_regex` and `ignore_regex`, and requires Zellij >= 0.45. `triggers` still works but is deprecated. See the [CHANGELOG](CHANGELOG.md#030---unreleased) for the migration table and the full list of changes.

## A different approach to keybindings

**The default approach.** Zellij's stock keybindings are modal. `Ctrl+p` enters Pane mode, then `h` moves focus left, then `Esc` returns to Normal. `Ctrl+s` enters Scroll mode, then `u` scrolls half a page. Every action is a sequence, and the sequences exist because Zellij cannot know whether `Ctrl+h` was meant for it or for the program in the pane. Zellij does offer a Locked mode that passes everything through, but *you* have to toggle it (`Ctrl+g`) every time you open or leave an editor.

**The autolock approach.** Because the plugin guarantees Zellij is Locked whenever a matching program owns the pane, the ambiguity is gone:

- Start from `keybinds clear-defaults=true` and define **single-key** bindings in `shared_except "locked"`. They apply at the shell prompt and are transparently passed through when Vim, fzf, etc. are running.
- Dedicate one modifier layer (the example below uses `Alt`) to Zellij actions that must work even while locked. Put those in `locked { ... }`.
- Keep a manual escape hatch (`Alt .` to flip the lock, `Alt Shift .` to disable autolock and flip the lock) for programs your regex does not cover.

| Action | Default Zellij | With autolock |
|--------|----------------|---------------|
| Move focus left | `Ctrl+p`, `h` | `Ctrl+h` |
| Scroll half page up | `Ctrl+s`, `u` | `Ctrl+u` |
| Give the keyboard to Vim | `Ctrl+g` (by hand) | automatic |
| Take the keyboard back | `Ctrl+g` (by hand) | automatic, or `Alt+.` |

See [Keybinding comparison](#keybinding-comparison) for the full action-by-action table.

The essentials, from a working config:

```kdl
keybinds clear-defaults=true {
    // Keybindings specific to 'Normal' mode.
    normal {
        // Intercept `Enter`.
        bind "Enter" {
            // Passthru `Enter`.
            WriteChars "\r";
            // Invoke autolock to immediately assess proper lock state.
            // (This provides a snappier experience compared to
            // solely relying on `reaction_seconds` to elapse.)
            MessagePlugin "autolock" {};
        }
        //...
    }
    // Keybindings specific to 'Locked' mode.
    locked {
        bind "Alt ." {
            SwitchToMode "normal"
        }
        bind "Alt Shift ." {
            MessagePlugin "autolock" { payload "toggle"; }
            SwitchToMode "normal"
        }
        //...
    }
    // Keybindings shared across all modes, except 'Locked'.
    shared_except "locked" {
        // Put keybindings here if they conflict with Vim or others.

        bind "Alt ." {
            SwitchToMode "locked"
        }
        bind "Alt Shift ." {
            MessagePlugin "autolock" { payload "toggle"; }
            SwitchToMode "locked"
        }

        // Vim-like bindings:
        bind "Ctrl h" { MoveFocus "left"; }
        bind "Ctrl j" { MoveFocus "down"; }
        bind "Ctrl k" { MoveFocus "up"; }
        bind "Ctrl l" { MoveFocus "right"; }
        bind "Ctrl b" { PageScrollUp; }
        bind "Ctrl f" { PageScrollDown; }
        bind "Ctrl u" { HalfPageScrollUp; }
        bind "Ctrl d" { HalfPageScrollDown; }

        //...
    }
    //...
}
```

The `Enter` binding is optional but recommended. Without it, the plugin notices a new command only after `reaction_seconds` elapses; with it, pressing `Enter` to launch a program locks Zellij immediately.

## Keybinding comparison

The table below compares Zellij's stock keybindings (`zellij setup --dump-config`) with the sample autolock configuration this README recommends. Two things stand out:

- **Sequences become single keys.** Almost every action in the default config is a two-step sequence through a mode (`Ctrl+p` for Pane, `Ctrl+t` for Tab, `Ctrl+s` for Scroll, ...). The sample binds each action to one chord instead, and drops the Pane, Tab, Move, Session, and Tmux modes entirely.
- **`Alt` works everywhere, `Ctrl` works at the prompt.** Bindings in `shared { ... }` are active in every mode, including Locked, so they keep working inside Vim. Bindings in `shared_except "locked"` only apply at the shell prompt, which is what lets the sample reuse keys that Vim, fzf, and less also want, such as `Ctrl+h` (Move mode in the default config), `Ctrl+b` (Tmux mode), `Ctrl+d`, `Ctrl+f`, and `Ctrl+u`.

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
| Toggle fullscreen pane | `Ctrl+p`, `f` | `Alt+m` (also toggles autolock) | yes |
| Move pane left / down / up / right | `Ctrl+h`, `h` / `j` / `k` / `l` | `Alt+Shift+h` / `j` / `k` / `l` | yes |
| Show / hide floating panes | `Alt+f` | `Alt+Space` | yes |
| Embed or float the focused pane | `Ctrl+p`, `e` | `Alt+f` | yes |
| Pin floating pane | `Ctrl+p`, `i` | `Alt+Enter` | yes |
| Enter Resize mode | `Ctrl+n` | `Alt+r` | yes |
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
| Keybinding cheat sheet ([zellij_forgot](https://github.com/karimould/zellij-forgot)) | — | `Alt+Shift+/` | yes |

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

Install the companion Vim plugin [***zellij.vim***](https://github.com/fresh2dev/zellij.vim) to unlock seamless navigation across Zellij Panes and Vim windows.

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
