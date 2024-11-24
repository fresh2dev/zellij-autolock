# zellij-autolock

*zellij-autolock* is an Zellij plugin that automatically switches between Zellij's "Normal" and "Locked" modes by inspecting the command running within the focused Zellij pane.

I built *zellij-autolock* in pursuit of seamless navigation between Zellij panes and Vim windows. It works well for Vim, Neovim, and with other CLI applications that use keymaps that conflict with Zellij's keymaps including Helix, FZF, and more.

> Note: When using with [Neo]vim, you'll also want to install this companion Vim plugin: [***zellij.vim***](https://github.com/fresh2dev/zellij.vim)

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

Download the wasm file from the [releases page](https://github.com/fresh2dev/zellij-autolock/releases). Save it to your Zellij config path (e.g., `~/.config/zellij/plugins/zellij-autolock.wasm`). You will reference this path when defining the plugin in your Zellij config.

> Note: Zellij >= 0.41 is also required.

## Config

This is a "headless" Zellij plugin; it has no UI. Once activated, this plugin responds to Zellij events ( `TabUpdate`, `PaneUpdate`, and `InputReceived` ) by examining the process running within the focused Zellij pane. If the running process is in set of `triggers`, Zellij is then set to "Locked" mode. Otherwise it is unlocked (i.e., set to "Normal").

> Note: this plugin reacts to user input events, but it does not (and cannot) read user input.

### Example `config.kdl`

```kdl
plugins {
    // Define the "autolock" plugin.
    autolock location="file:~/.config/zellij/plugins/zellij-autolock.wasm" {
        // Enabled at start?
        is_enabled true
        // Lock when any open these programs open.
        triggers "nvim|vim|git|fzf|zoxide|atuin"
        // Reaction to input occurs after this many seconds. (default=0.3)
        // (An existing scheduled reaction prevents additional reactions.)
        reaction_seconds "0.3"
        // Print to Zellij log? (default=false)
        print_to_log true
    }
    //...
}
// Load this "headless" plugin on start.
load_plugins {
    autolock
}
keybinds {
    // Keybindings specific to 'Normal' mode.
    normal {
        // Intercept `Enter`.
        bind "Enter" {
            // Passthru `Enter`.
            WriteChars "\u{000D}";
            // Invoke autolock to immediately assess proper lock state.
            // (This provides a snappier experience compared to
            // solely relying on `reaction_seconds` to elapse.)
            MessagePlugin "autolock" {};
        }
        //...
    }
    // Keybindings specific to 'Locked' mode.
    locked {
        bind "Alt z" {
            // Disable the autolock plugin.
            MessagePlugin "autolock" {payload "disable";};
            // Unlock Zellij.
            SwitchToMode "Normal";
        }
        //...
    }
    // Keybindings shared across all modes.
    shared {
        bind "Alt Shift z" {
            // Enable the autolock plugin.
            MessagePlugin "autolock" {payload "enable";};
        }
        //...
    }
    // Keybindings shared across all modes, except 'Locked'.
    shared_except "locked" {
        // Put keybindings here if they conflict with Vim or others.

        bind "Alt z" {
            // Disable the autolock plugin.
            MessagePlugin "autolock" {payload "disable";};
            // Lock Zellij.
            SwitchToMode "Locked";
        }

        bind "Ctrl h" {
            MoveFocusOrTab "Left";
        }
        bind "Ctrl l" {
            MoveFocusOrTab "Right";
        }
        bind "Ctrl j" {
            MoveFocus "Down";
        }
        bind "Ctrl k" {
            MoveFocus "Up";
        }

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

When `MessagePlugin` is called without a payload, immediate assessment of the currently running command occurs. This is useful in conjunction with the `Enter` key to provide a snappier experience. Additionally, a payload can be provided to enable, disable, or toggle the autolock mechanism.

- `MessagePlugin "autolock" {};` \<- immediately trigger an assessment of the current pane.
- `MessagePlugin "autolock" {payload "disable"};` \<- disable autolock
- `MessagePlugin "autolock" {payload "enable"};` \<- enable autolock
- `MessagePlugin "autolock" {payload "toggle"};` \<- toggle autolock

## Troubleshooting

If you experience issues with the plugin, first try opening a fresh Zellij session. If that doesn't work, clear the Zellij cache and restart Zellij (`rm -rf ~/.cache/zellij` on Linux; `rm -rf ~/Library/Caches/org.Zellij-Contributors.Zellij` on macOS)

[Zellij logs](https://zellij.dev/documentation/plugin-api-logging) are viewable here on Linux:

```sh
tail -f /tmp/zellij-$(id -u)/zellij-log/zellij.log
```

On MacOS, you'll have to hunt for it in the directory `/var/folders`. This is what I use to easily find and tail the Zellij log on MacOS:

```sh
find /var/folders -type f -name 'zellij.log' -exec tail -f {} \; 2>/dev/null
```

## Shoutouts

- [zellij-org/zellij](https://github.com/zellij-org/zellij)
- [hiasr/vim-zellij-navigator](https://github.com/hiasr/vim-zellij-navigator)
- [christoomey/vim-tmux-navigator](https://github.com/christoomey/vim-tmux-navigator)
- [dj95/zjstatus](https://github.com/dj95/zjstatus)
