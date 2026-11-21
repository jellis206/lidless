# Lidless (Rust)

A switch in your Mac's menu bar. Click it and your Mac stays awake, even with the lid shut. Click it again and it sleeps like normal. macOS 13 or later.

This is a Rust port of [alexiscodingbits/lidless](https://github.com/alexiscodingbits/lidless) by Alex McConnell (MIT), with some changes of its own, listed [below](#whats-different-from-the-original). The app icon is his.

## Install

There is no download. You build it on the Mac you'll use it on, which needs [Rust](https://rustup.rs) and `jq`:

```
scripts/bundle.sh --install  # builds, copies Lidless.app to /Applications and opens it
scripts/bundle.sh            # builds dist/Lidless.app only
```

The build is signed for local use and is not notarised, so a copy sent to another Mac won't open. Each person builds their own.

## Using it

| Menu bar | What it means |
|---|---|
| Closed eye | Normal. Your Mac sleeps when it idles or you close the lid. |
| Open eye | Awake. Your Mac keeps running, even with the lid shut. |

Left-click switches. Right-click opens the menu, which states the current mode in words and holds the options.

The first click shows the macOS admin password prompt, once. After that every click is instant.

## What it does

- **Keeps the Mac awake with the lid shut.** `caffeinate` and most keep-awake apps only stop idle sleep; closing the lid still sleeps the Mac. Lidless flips `pmset disablesleep`, which blocks both. With the eye open you don't need `caffeinate` as well.
- **Fades the screen when the lid shuts.** While awake, shutting the lid fades the built-in screen to black over about a second, and opening it fades back to the brightness it had. External monitors are not touched.
- **Switches itself off when it isn't safe.** If the Mac gets hot, or is on battery at 20% or less, Lidless goes back to normal sleep and tells you. It won't switch on in those states either. You can turn this off in the menu under **Auto-off When Hot or Below 20% Battery**, but you probably shouldn't: a Mac running in a closed bag gets hot and drains its battery.
- **Restores everything on quit.** Quitting puts normal sleep back, and puts the screen brightness back if the lid is shut.
- **Open at Login**, from the menu. It is off until you turn it on.

## Your Mac still locks

Lidless changes when your Mac sleeps, not when it locks. With the eye open:

- Walk away, or close the lid, and the screen turns off after your usual display-sleep time. The Mac then locks the way it normally does when the display turns off (System Settings → Lock Screen → *Require password after screen saver begins or display is turned off*).
- Lock it yourself (Control-Command-Q, or Apple menu → Lock Screen) and it locks straight away.
- Either way, everything keeps running behind the lock screen: builds, downloads, scripts, network connections.

So the Mac stays secure while it keeps working. The one gap is the time between closing the lid and the display turning off, when the Mac is awake and not yet locked. If that matters where you are, lock it before you close it.

## How the switch works

Changing `pmset disablesleep` needs root. The first click uses the admin prompt to flip the setting and install one sudoers rule at `/etc/sudoers.d/lidless`:

```
you ALL=(root) NOPASSWD: /usr/bin/pmset -a disablesleep 0, /usr/bin/pmset -a disablesleep 1
```

That rule lets your user run those two exact commands without a password, and nothing else. It is checked with `visudo` before it goes live. On a managed Mac, a new file in `/etc/sudoers.d` may show up in endpoint monitoring.

There is no networking code. The binary links only Apple's own frameworks.

macOS has no public API for screen brightness, so the fade uses Apple's private DisplayServices framework, loaded at runtime. If a macOS update removes it, the fade stops working and nothing else does.

## What's different from the original

Added:

- The screen fade on lid close and open.
- An eye for the menu-bar icon, open when awake and closed when normal, instead of a moon with a slash through it.
- Quitting restores normal sleep. In the original, quitting while awake left the Mac unable to sleep with no safety cutoff running.
- Unit tests for the `pmset` parsing, the sudoers rule and the fade.

Changed:

- Written in Rust rather than Swift.
- It does not add itself to Open at Login on first launch. You opt in from the menu.
- Battery level is read from `pmset -g batt` rather than IOKit.
- Built and signed locally, with no notarised download.

Removed:

- The About menu item.
- Cleanup of the older `sleeptoggle` sudoers rule from before the original was renamed.

## Known issues

- The hover tooltip on the menu-bar icon doesn't appear on macOS 27. Right-click to read the current mode.

## Develop

```
cargo test               # parsing, sudoers-rule and fade tests
cargo test -- --ignored  # hardware check: reads the lid and dips the screen briefly
```

## Uninstall

1. Right-click the icon and choose **Ask for Password Again…**. This deletes the sudoers rule.
2. Quit Lidless and delete the app.

By hand: `sudo rm /etc/sudoers.d/lidless && sudo pmset -a disablesleep 0`.

## Licence

MIT. Original work © 2026 Alex McConnell.
