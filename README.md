# zellij-autolock

*zellij-autolock* is an experimental Zellij plugin that automatically switches between Zellij's "Normal" and "Locked" modes depending on the process running within the focused Zellij pane.

I built *zellij-autolock* in pursuit of seamless navigation between Zellij panes and Vim windows. It works well for Vim, Neovim, and with other CLI applications that use keymaps that conflict with Zellij's keymaps including Helix, FZF, and more.

> Note: When using with [Neo]vim, you'll also want to install this Vim plugin: [***zellij.vim***](https://github.com/fresh2dev/zellij.vim)

## Demo

Here is a demonstration of how you can seamlessly navigate through Zellij panes running Vim, Neovim, Helix, FZF, and more.

<video autoplay="false" controls="controls" style="width: 800px;">
  <source src="https://img.fresh2.dev/1716528665751_11894996682.webm" type="video/webm"/>
  <p><i>This page does not support webm video playback.</i></p>
  <p><i><a href="https://img.fresh2.dev/1716528665751_11894996682.webm" target="_blank">Click here to watch the demo recording.</a></i></p>
</video>
<p><b><i><a href="https://img.fresh2.dev/1716528665751_11894996682.webm" target="_blank">Open full screen demo recording.</a></i></b></p>

Notice how the Zellij mode ( "Normal" or "Locked" in the top-right corner ) automatically toggles depending which process is running within the focused Zellij pane. This allows you to use the same mappings ( `Ctrl+h/j/k/l` ) to navigate between Zellij panes, Vim windows, FZF results, and more.

## Install / Config

> Note: Zellij >= 0.40.1 is required.

This is a "headless" Zellij plugin; it has no UI. It is activated by pressing the `Enter` key in Zellij's "Normal" mode. Once activated, this plugin responds to Zellij events ( `TabUpdate` and `PaneUpdate` ) by examining the process running within the focused Zellij pane. If the running process is in either list `trigger` or `watch_trigger`, Zellij is then set to "Locked" mode. Otherwise it is unlocked (i.e., set to "Normal").

I use the following Zellij config to intercept the "Enter" key, pass it through, then launch this plugin -- (an excerpt from [my dotfiles](https://github.com/fresh2dev/dotfiles)):

```kdl
plugins {
    // Define the "autolock" plugin.
    autolock location="https://github.com/fresh2dev/zellij-autolock/releases/latest/download/zellij-autolock.wasm" {
        triggers "nvim|vim"  // Lock when any open these programs open. They are expected to unlock themselves when closed (e.g., using zellij.vim plugin).
        watch_triggers "fzf|zoxide|atuin"  // Lock when any of these open and monitor until closed.
        watch_interval "1.0"  // When monitoring, check every X seconds.
    }
    //...
}
keybinds {
    normal {
        bind "Enter" {  // Intercept `Enter`.
            WriteChars "\u{000D}";  // Passthru `Enter`.
            MessagePlugin "autolock" {};  // Activate the autolock plugin.
        }
        // Note: You may want to bind/intercept/relay other keys to activate this plugin,
        // like `Ctrl+r` which opens shell history in Atuin / FZF. For example:
        // bind "Ctrl r" {  // Intercept `Ctrl+r`.
        //     WriteChars "\u{0012}";  // Passthru `Ctrl+r`
        //     MessagePlugin "autolock" {};  // Activate the autolock plugin.
        // }
    }
    //...
    shared_except "locked" {
        // Put keybindings here if they conflict with Vim or others.

        bind "Ctrl h" { MoveFocus "Left"; }
        bind "Ctrl l" { MoveFocus "Right"; }
        bind "Ctrl j" { MoveFocus "Down"; }
        bind "Ctrl k" { MoveFocus "Up"; }

        // bind "Ctrl d" { HalfPageScrollDown; }
        // bind "Ctrl u" { HalfPageScrollUp; }

        // bind "Ctrl f" { PageScrollDown; }
        // bind "Ctrl b" { PageScrollUp; }

        //...
    }
    //...
}
```

The `triggers` setting allows a pipe-separated (`|`) list of CLI commands that will trigger Zellij's "Locked" mode.

This plugin **can lock** Zellij when it encounters one of these commands, but it **cannot unlock** when the command exits. So, commands provided to `triggers` are expected to unlock Zellij on close. In the context of [Neo]vim, this is made possible with the Vim plugin [zellij.vim](https://github.com/fresh2dev/zellij.vim).

The `watch_triggers` setting accounts for CLI commands that have no way of unlocking Zellij on close. When *zellij-autolock* encounters one of these commands, it puts Zellij into "Locked" mode and starts a timer that evaluates every `<watch_interval>` seconds. When the command exits, Zellij will unlock after the timer performs a cycle. This is intended for transient commands that benefit from `Ctrl+h/j/k/l` nav, like `fzf`, `zoxide`, and `atuin`.

## Troubleshooting

If you experience issues with the plugin, first try opening a fresh Zellij session. If that doesn't work, clear the Zellij cache and restart Zellij (`rm -rf ~/.cache/zellij` on Linux; `~/Library/Caches/org.Zellij-Contributors.Zellij` on macOS)

[Zellij logs](https://zellij.dev/documentation/plugin-api-logging) are viewable here (on Linux):

```sh
tail -f /tmp/zellij-$(id -u)/zellij-log/zellij.log
```

## Future

Today, this plugin uses Zellij CLI commands to determine the process running within the focused Zellij pane (there is a report of terminal flashing [#3](https://github.com/fresh2dev/zellij-autolock/issues/3) related to this implementation). When it is possible to use the Zellij API to make the distinction, I plan to refactor.

This project is experimental and exists as a proof-of-concept. This is my first round of writing a Zellij plugin, or a project in Rust for that matter. If this project were to thrive, it would not be without community contributions. If this plugin solves [the Zellij / Vim navigation problem (#967)](https://github.com/zellij-org/zellij/issues/967), I dream that it'd just be baked into the Zellij project.

## Shoutouts

- [zellij-org/zellij](https://github.com/zellij-org/zellij)
- [hiasr/vim-zellij-navigator](https://github.com/hiasr/vim-zellij-navigator)
- [christoomey/vim-tmux-navigator](https://github.com/christoomey/vim-tmux-navigator)
- [dj95/zjstatus](https://github.com/dj95/zjstatus)
