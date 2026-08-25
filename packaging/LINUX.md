# Hark on Linux

Hark works the same on X11 and Wayland, on GNOME, KDE, XFCE, Cinnamon and the
wlroots compositors. One setup step is required before first use, and it is the
same step on every distribution.

## Install

Download the package for your distribution from the
[latest release](https://github.com/BoardPandas/Hark/releases/latest).

```bash
sudo apt install ./Hark-<version>-linux-x64.deb          # Debian, Ubuntu, Mint, Pop!_OS
sudo dnf install ./Hark-<version>-linux-x64.rpm          # Fedora, RHEL, openSUSE
sudo pacman -U Hark-<version>-linux-x64.pkg.tar.zst      # Arch, Manjaro, EndeavourOS, CachyOS
```

A portable `.tar.gz` is also published for distributions none of those cover;
see [Portable tarball](#portable-tarball) below, which has manual steps the
packages do for you.

**Requires glibc 2.39 or newer** (Ubuntu 24.04+, Debian 13+, Fedora 40+). The
binary is built on Ubuntu 24.04, so it will not run on Debian 12 or Ubuntu
22.04 — those need a build from source (`cargo build --release -p hark-app`).

## The one required step: the `input` group

```bash
sudo usermod -aG input $USER
```

**Then log out and log back in.** Opening a new terminal is not enough — Linux
fixes a process's group membership at login, so a shell started before the
change still cannot see the new group. `id -nG` should list `input` afterwards.

### Why

Hark is a push-to-talk app, so it has to know when you are holding a key in
*another* application's window, and then put text into that window. On Linux
that means two things:

| What Hark does | Through | Needs |
|---|---|---|
| Sees the push-to-talk chord | reading `/dev/input/event*` | membership of `input` |
| Pastes the transcript | writing `/dev/uinput` | membership of `input`, plus Hark's udev rule |

`/dev/input/event*` is already owned by `root:input` on every mainstream
distribution, so the group membership is all that is needed there. `/dev/uinput`
is created `root:root` by the kernel, which is why the packages install
`/usr/lib/udev/rules.d/70-hark-uinput.rules` to hand it to the same group — one
membership covers both.

Reading the input devices directly is what makes Hark work under Wayland at
all. The X11 APIs that other global-hotkey tools use are invisible to a Wayland
compositor, and Wayland is the default session on GNOME and KDE.

Hark reads key **events**, it never grabs a device, and it never intercepts or
swallows a keystroke — every key still reaches the application you are typing
into. It watches for the configured chord and ignores everything else.

### If you skip it

Hark starts, opens its window, and says so:

> cannot read any keyboard under /dev/input. Hark needs permission to watch for
> the push-to-talk chord: add yourself to the "input" group
> (`sudo usermod -aG input $USER`), then log out and back in.

## Storing your API key

Hark keeps your transcription provider's API key in the Secret Service, which
means `gnome-keyring` or `kwallet` — whichever your desktop already runs. If
neither is available, set the key in the environment instead:

```bash
export HARK_STT_KEY=your-key-here
```

## Portable tarball

The `.tar.gz` contains the same binary and support files but installs nothing.
After extracting `Hark-<version>-linux-x64.tar.gz` and entering the directory:

```bash
sudo install -Dm755 hark /usr/local/bin/hark
sudo install -Dm644 70-hark-uinput.rules /etc/udev/rules.d/70-hark-uinput.rules
sudo install -Dm644 hark-uinput.conf /etc/modules-load.d/hark-uinput.conf
sudo install -Dm644 hark.desktop /usr/local/share/applications/hark.desktop
sudo install -Dm644 hark.svg /usr/local/share/icons/hicolor/scalable/apps/hark.svg
sudo modprobe uinput && sudo udevadm control --reload-rules && sudo udevadm trigger
```

Then do the `input` group step above.

## Updating

Your package manager handles it — `apt upgrade`, `dnf upgrade`, `pacman -Syu`.
Hark tells you when a new version exists and links to the release, but it never
replaces its own binary on Linux: that file belongs to the package manager, and
overwriting it would fail the package's integrity checks and be reverted by the
next upgrade.

## Known platform differences

These are the places Linux cannot match Windows exactly. Everything else —
dictation, cleanup, voices, the spellbook, invocations, history, statistics,
on-device transcription, the recording overlay, launch-at-login — behaves the
same.

- **`swallow_lock_keys` does nothing.** On Windows, a chord containing Caps
  Lock or Scroll Lock can suppress the lock's toggle. Doing that on Linux would
  need `EVIOCGRAB`, which takes the keyboard *exclusively* and would stop every
  other keystroke reaching your applications. Hark observes instead, so a lock
  key inside a chord still toggles. The default chord contains no lock key.
- **Tray tooltips are not shown.** The StatusNotifierItem protocol that Linux
  panels use has no tooltip. The tray menu and the Hark window carry the same
  status text.
- **The tray icon has a "Show Hark" item** instead of responding to a
  double-click, because the protocol delivers no click events.
- **Typing mode is ASCII-only under Wayland.** The `Type` injection strategy
  synthesizes individual keystrokes, and Wayland exposes no keyboard map to
  applications, so Hark assumes a US layout. Characters outside ASCII — the
  em dashes and curly quotes the cleanup pass likes to produce — are refused
  rather than mangled. The default `Clipboard` strategy has no such limit and
  carries any character; use it unless a specific field rejects pastes.
- **Ctrl+V assumes V is where a US keyboard puts it, under Wayland only.**
  uinput speaks key positions and Wayland will not tell an application what the
  active layout is, so on Dvorak or Colemak the paste chord lands on the wrong
  key. X11 sessions have no such problem — Hark uses XTEST there, which
  resolves the layout properly.

## Troubleshooting

**Nothing happens when I hold the chord.** Check `id -nG` includes `input`. If
it does, look at `~/.local/share/hark/hark.log` — the listener logs how many
keyboards it opened at startup.

**The GNOME overview (or the KDE launcher) opens when I use the chord.** The
default chord is Left Ctrl + Left Super, and some desktops open their launcher
when Super is released. Hark observes keys rather than swallowing them — that
is deliberate, and it is what keeps every other shortcut on your machine
working — so it cannot suppress that. Pick a chord your desktop does not bind,
under **Settings → Shortcut**; `Ctrl+Alt` and the F13–F24 keys are good choices,
and a function key above F12 is bound by nothing at all.

**Dictation works but nothing is pasted.** Check that `/dev/uinput` exists
(`ls -l /dev/uinput`) and is group `input` with mode `0660`. If it is missing,
`sudo modprobe uinput`. If the group is wrong, `sudo udevadm control
--reload-rules && sudo udevadm trigger`.

**Nothing is pasted, and I use Dvorak or Colemak on Wayland.** See the
layout note above. Switching the injection strategy will not help; an X11
session will.

**The tray icon is missing on GNOME.** GNOME removed built-in tray support;
install the AppIndicator extension. Hark still works without it — the window is
reachable from your launcher.

**Hark says it cannot save my API key.** No Secret Service is running. Install
`gnome-keyring` or `kwallet`, or use `HARK_STT_KEY`.
