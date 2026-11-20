# Lidless (Rust)

A switch in your Mac's menu bar. Click it and your Mac stays awake, even with the lid shut. Click it again and it sleeps like normal. macOS 13 or later.

This is a Rust port of [alexiscodingbits/lidless](https://github.com/alexiscodingbits/lidless) by Alex McConnell (MIT). The icon is his.

## Build

```
scripts/bundle.sh            # dist/Lidless.app, signed ad hoc
scripts/bundle.sh --install  # same, then copies it to /Applications and opens it
cargo test                   # parsing and sudoers-rule tests
```

The build is for this Mac only: it is not notarised, so Gatekeeper will refuse it on another machine.

## How it works

Left-click switches. Right-click opens the menu.

Changing `pmset disablesleep` needs root. The first click shows the macOS admin prompt once, and uses it to flip the setting and install `/etc/sudoers.d/lidless`:

```
you ALL=(root) NOPASSWD: /usr/bin/pmset -a disablesleep 0, /usr/bin/pmset -a disablesleep 1
```

That rule allows those two exact commands and nothing else. It is checked with `visudo` before it goes live. After that, every click is instant. There is no networking code.

## Safety

With Lidless on, your Mac keeps running in a closed bag, so it switches itself off (and won't switch on) when the Mac is hot or on battery at 20% or less. The right-click menu can disable that.

The cutoff only runs while Lidless does, so quitting Lidless restores normal sleep.

## Differences from the original

- Quitting restores normal sleep.
- It does not add itself to Open at Login on first launch. Use the menu.
- Battery level is read from `pmset -g batt` rather than IOKit.
- No About item and no legacy `sleeptoggle` rule cleanup.

## Uninstall

1. Right-click the icon and choose **Ask for Password Again…**. This deletes the sudoers rule.
2. Quit Lidless and delete the app.

By hand: `sudo rm /etc/sudoers.d/lidless && sudo pmset -a disablesleep 0`.
