# ConsoleDeck

[![Build](https://github.com/mike623/consoledeck/actions/workflows/build.yml/badge.svg)](https://github.com/mike623/consoledeck/actions/workflows/build.yml)
![macOS 13+](https://img.shields.io/badge/macOS-13%2B-black)
![Windows 10/11](https://img.shields.io/badge/Windows-10%2F11-blue)

Turn a [Console Deck V2](https://makerworld.com/en/models/1717141-console-deck-v2) into a macro pad for macOS and Windows: a tray app that listens to the deck over USB and runs your actions, with per-app profiles, long and double press.

[Features](#features) • [Tested hardware](#tested-hardware) • [Getting started](#getting-started) • [Actions](#actions) • [Configuration](#configuration) • [Command line](#command-line-deckpy)

## Features

- **Tray app for macOS and Windows** (Rust + [Tauri](https://tauri.app)), no Python or service needed at runtime.
- **Wire it however you like**: a calibration window maps whatever pins your buttons use to the 3-3-4 layout.
- **Per-app profiles**: buttons change with the app in front, and fall back to the default profile.
- **Gestures**: press, long press and double press on every button.
- **Many action types**: open a URL or app, run a shell script, press a key combo, type text, switch input language, lock, sleep, volume and more.
- **Auto-reconnect** when the deck is unplugged or the board reboots.
- **Start at login** toggle in the menu.

## Tested hardware

| Part | Model |
| --- | --- |
| Microcontroller | Arduino Nano, **ATmega328P** (new bootloader) |
| USB-serial chip | **FTDI FT232R** (`0403:6001`), shows up as `/dev/cu.usbserial-*` |
| Arduino FQBN | `arduino:avr:nano:cpu=atmega328` |
| Enclosure | Console Deck V2: 10 buttons in a 3-3-4 layout + rotary encoder |
| Host | macOS 15 on Apple silicon (Windows builds are untested on hardware) |

The app (and `deck.py doctor`) also recognises CH340, CP2102, ATmega16U2 (Uno/Nano) and Nano Every USB chips, but only the board above has been tested. On Windows, FTDI drivers come from Windows Update; CH340 clones may need the [WCH driver](https://www.wch-ic.com/downloads/CH341SER_EXE.html).

> [!NOTE]
> ESP32 boards are not tested. The firmware only needs `INPUT_PULLUP` pins and serial at 115200 baud, so porting should mostly mean editing the `pins[]` list in `deck_debug/deck_debug.ino`.

## Getting started

### 1. Flash the firmware

Upload [`deck_debug/deck_debug.ino`](deck_debug/deck_debug.ino) with the Arduino IDE or `arduino-cli`:

```bash
arduino-cli compile --fqbn arduino:avr:nano:cpu=atmega328 deck_debug
arduino-cli upload  --fqbn arduino:avr:nano:cpu=atmega328 -p /dev/cu.usbserial-XXXX deck_debug
```

The firmware enables pull-ups on D2–D13 and A0–A5, prints `D7 PRESSED` / `D7 released` at 115200 baud, and answers `?` with `PONG consoledeck`.

> [!TIP]
> If the upload fails with `not in sync: resp=0x00`, the board's auto-reset isn't working. Retry the upload while tapping the Nano's **RESET** button every couple of seconds.

### 2. Install the app

Download from [Releases](https://github.com/mike623/consoledeck/releases) (or the latest [Build](https://github.com/mike623/consoledeck/actions/workflows/build.yml) run):

- **macOS**: unzip `ConsoleDeck-macos.zip` and move `ConsoleDeck.app` to Applications.
- **Windows**: run `ConsoleDeck-windows-setup.exe`. ConsoleDeck appears in the tray (check the `^` overflow).

> [!IMPORTANT]
> CI builds are not notarized or code-signed.
> - **macOS** blocks the first launch. Either run `xattr -dr com.apple.quarantine /Applications/ConsoleDeck.app`, or try to open it once and then click **Open Anyway** in System Settings → Privacy & Security.
> - **Windows** SmartScreen warns about an unknown publisher. Click **More info** → **Run anyway**.

To build it yourself you need [Rust](https://rustup.rs) and Node.js (for the Tauri CLI), and the bundled plugins:

```bash
git submodule update --init   # app/plugins: the plugins shipped inside the app
app/build.sh           # builds the app (Mac) or installer (Windows) into app/src-tauri/target/release/bundle
app/build.sh install   # Mac: also copies it to ~/Applications and launches it
cd app/src-tauri && cargo run    # run from source
cd app/src-tauri && cargo test   # tests
```

On a Mac, `build.sh` signs with your Apple Development identity when it finds one, so the Accessibility permission survives rebuilds.

### 3. Calibrate and assign actions

1. Click the ConsoleDeck icon in the menu bar (tray on Windows) → **Calibrate Buttons…**, then press each button in the order shown.
2. Open **Edit Actions…**, pick a button and gesture, and choose an action. Use **Test** to try it.
3. Add a per-app profile with **+** (recently used apps, or **Choose App…**); buttons left on **Use Default** fall back to the default profile. Profiles are keyed by bundle ID on macOS and by exe name (e.g. `chrome.exe`) on Windows.

> [!NOTE]
> On macOS, **Press Keys**, **Type Text**, **Type Password**, **Lock Screen** and the volume keys need Accessibility access. The Actions window shows an **Allow…** button when it's missing. Windows needs no permission, but it can't send keys to apps running as administrator unless ConsoleDeck does too.

## Actions

Every action except Repeat Keys, Open ConsoleDeck Editor and Show Cheat Sheet is a plugin from
[consoledeck-plugins](https://github.com/mike623/consoledeck-plugins). The ones below ship inside the app and are
installed on first launch; **Plugins…** (tray menu, or the Actions window) browses the registry, installs, updates
and removes them. Plugins that run commands show them and ask before installing. To write one, see that repo's README.

| Type | What it does |
| --- | --- |
| Open URL | Opens a link in the default browser |
| Open App | Launches or focuses an app (Windows: a path from **Choose…**, or anything `start` understands) |
| Run Script | Runs a command or script path: bash on macOS, `cmd` on Windows |
| Press Keys | Sends a recorded key combo, e.g. `⌘⇧4` (⌘ is the Windows key on Windows) |
| Repeat Keys | Toggles pressing a key combo every N seconds; stop it from the menu |
| Type Text | Types a string, newlines become Return |
| Type Password | Types a password. Stored AES-256-GCM encrypted in `deck.json` (key in `secret.key` beside it) and shown masked |
| Input Language | Switches to a specific input source / keyboard language |
| Toggle VPN (Mac) | Connects a [Tunnelblick](https://tunnelblick.net) VPN, or disconnects it when it's up |
| System | Lock Screen, Sleep, Turn Display Off, Screen Saver, Next Input Language, Mute, Volume Up/Down, Log Out, Restart, Shut Down, Open ConsoleDeck Editor, Show Cheat Sheet (hold to see every button's actions) |

On macOS, Log Out, Restart and Shut Down go through macOS's own confirmation dialog, so a stray press can't lose work. Windows has no such dialog, so Restart and Shut Down wait 15 seconds with a warning (`shutdown /a` cancels). Sleep on Windows hibernates instead if hibernation is enabled.

## Configuration

Settings live in `~/.config/consoledeck/deck.json` on macOS and `%APPDATA%\ConsoleDeck\deck.json` on Windows (older versions' settings are moved there on first launch) (menu → **Show Config**). The app re-reads it on every press, so hand edits apply immediately. Timing knobs without UI (defaults shown):

```json
{
  "longPressMs": 500,
  "doublePressMs": 300,
  "repeatSeconds": 60
}
```

> [!WARNING]
> Only one program can hold the serial port. Quit ConsoleDeck (and close the Arduino Serial Monitor) before uploading firmware or running `deck.py`.

## Command line (`deck.py`)

A terminal alternative to the app on macOS, sharing the same config file. Needs [uv](https://docs.astral.sh/uv/).

```bash
uv run deck.py                            # interactive menu: calibrate, assign actions
uv run deck.py doctor                     # check USB chip, firmware ping, config, service
uv run deck.py service install|uninstall|status   # run as a LaunchAgent instead of the app
uv run python test_deck.py                # tests
```

Set `DECK_PORT` to override the auto-detected `/dev/cu.usbserial*` / `/dev/cu.usbmodem*` port (the app honours it too, e.g. `DECK_PORT=COM5` on Windows). The CLI supports the default profile with URL, app and script actions only.

## Project layout

```
deck_debug/   Arduino firmware (pin finder + health ping)
app/          Tauri app: Rust in src-tauri/, windows in ui/, build.sh
app/plugins/  consoledeck-plugins submodule, bundled into the app
deck.py       terminal controller, doctor and LaunchAgent service
```
