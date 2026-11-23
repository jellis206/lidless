<p align="center"><img src="assets/icon-1024.png" width="150" alt="Lidless icon"></p>

<h1 align="center">Lidless</h1>

<p align="center"><b>Shut the lid. Keep the Mac running.</b></p>

One eye in your menu bar. Click it open and your Mac stops sleeping, lid shut and all. Click it closed and it's a normal Mac again.

Your build keeps building. Your download keeps downloading. Your agents keep cooking. You go get lunch.

| | Mode | Close the lid and... |
|---|---|---|
| 👁 | **Awake** | it keeps running |
| 😌 | **Normal** | it sleeps |

Left-click to flip. Right-click for the menu.

## Get it

No download. You build your own, which takes about a minute. You need [Rust](https://rustup.rs) and Xcode's command line tools (`xcode-select --install`, if you somehow don't have them).

```
git clone https://github.com/jellis206/lidless && cd lidless
scripts/bundle.sh
```

That drops `Lidless.app` into `/Applications` and opens it. The first click asks for your Mac password, once. Every click after that is instant.

The build is signed for your Mac only, so don't AirDrop the app to a friend. Send them this repo.

## What you get

- **Lid-shut awake.** `caffeinate` only stops idle sleep; close the lid and you're asleep anyway. Lidless blocks both, so you can retire the `caffeinate` tab.
- **A screen that fades out.** Shut the lid and the display eases down to black. Open it and it eases back to where you had it.
- **Lock on lid shut.** Turn on **Lock Screen When Lid Shuts** in the menu and closing the lid locks the Mac on the spot. Still awake, still working, just locked.
- **A fuse.** Mac running hot, or on battery at 20% or less? Lidless switches itself off and tells you, because a laptop cooking in a backpack is a bad day. You can disable this in the menu. Don't.
- **No mess on quit.** Quit and normal sleep comes back. So does your brightness.
- **Open at Login**, if you ask for it. It doesn't help itself.

## It still locks

Lidless changes when your Mac *sleeps*, not when it *locks*.

- Walk away or close the lid: the screen goes off after your usual display-sleep time, and the Mac locks like it always does.
- In a hurry: lock it yourself with `⌃⌘Q` and it locks now.
- Either way, everything keeps running behind the lock screen.

Locked and still working. That's the whole point.

One gap: between closing the lid and the screen turning off, the Mac is awake and not yet locked. Close it with **Lock Screen When Lid Shuts** turned on in the menu and the gap is gone.

## Security

Flipping sleep needs root, so here is exactly what Lidless does with it.

**Your password never touches Lidless.** macOS draws the prompt and checks it. Lidless hears "yes" or "no" and nothing more. There is no code that reads, stores or sends it.

**It gets two commands, forever.** That one prompt installs this, and only this, at `/etc/sudoers.d/lidless`:

```
you ALL=(root) NOPASSWD: /usr/bin/pmset -a disablesleep 0, /usr/bin/pmset -a disablesleep 1
```

Sleep on, sleep off, no password. Anything else, including those commands with one extra argument, is refused. The rule is checked with `visudo` before it goes live, the file is root-owned and read-only, and `pmset` itself is protected by macOS, so it can't be swapped for something nastier.

**Worst case:** another program running as you can toggle sleep. Annoying, not a way to root.

**It doesn't phone home.** No networking code. The app links only Apple's frameworks, runs every system tool by its full path, and is signed with the hardened runtime.

**Small supply chain.** Twelve crates, all pinned by checksum, clean under `cargo audit`.

The honest limits:

- You build and sign this yourself, so there's no Apple-verified identity behind it. Malware already running as you could swap the app for a fake. True of most apps you can modify.
- An awake Mac is a softer target than a sleeping one. Turn on **Lock Screen When Lid Shuts**, or lock it before you leave it.
- On a work Mac, your security tooling may notice the new sudoers file.

## Under the hood

Small enough to read in one sitting: three Rust files.

The fade and the lock use two private Apple calls (DisplayServices for brightness, `SACLockScreenImmediate` for the lock), because macOS has no public API for either. If an update removes the brightness one, the fade stops and everything else carries on. If it removes the lock one, the lock setting disappears from the menu instead of pretending to work.

## Versus the original

A Rust port of [Lidless](https://github.com/alexiscodingbits/lidless) by Alex McConnell. What's different here:

| | |
|---|---|
| **New** | Screen fade on lid close and open |
| **New** | Optional lock the moment the lid shuts |
| **New** | Eye icon: open for awake, closed for normal (was a moon with a slash) |
| **New** | Quitting restores normal sleep (the original left your Mac stuck awake) |
| **New** | Tests for the parsing, the sudoers rule and the fade |
| **Changed** | Rust instead of Swift |
| **Changed** | Open at Login is opt-in, not automatic |
| **Changed** | Build it yourself; no notarised download |
| **Gone** | The About menu item |

## Known quirks

- Hovering the icon shows no tooltip on macOS 27. Right-click instead; the menu says which mode you're in.

## Hack on it

```
cargo test                # the logic
cargo test -- --ignored   # the hardware: reads the lid, dips your screen for a blink
scripts/bundle.sh /tmp/x  # build into another folder; leaves your installed copy alone
```

## Uninstall

1. Right-click → **Ask for Password Again…** (removes the sudoers rule)
2. Quit, then bin the app

Or by hand: `sudo rm /etc/sudoers.d/lidless && sudo pmset -a disablesleep 0`

## Licence

[MIT](LICENSE).
