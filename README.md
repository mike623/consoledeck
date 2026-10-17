# ConsoleDeck

[![Build](https://github.com/mike623/consoledeck/actions/workflows/build.yml/badge.svg)](https://github.com/mike623/consoledeck/actions/workflows/build.yml)
![macOS 15+](https://img.shields.io/badge/macOS-15%2B-black)
![Apple silicon](https://img.shields.io/badge/Apple%20silicon-arm64-black)

Turn a [Console Deck V2](https://makerworld.com/en/models/1717141-console-deck-v2) into a macOS macro pad: a native menu bar app that listens to the deck over USB and runs your actions, with per-app profiles, long and double press.

[Features](#features) • [Tested hardware](#tested-hardware) • [Getting started](#getting-started) • [Actions](#actions) • [Configuration](#configuration) • [Command line](#command-line-deckpy)

## Features

- **Menu bar app** written in Swift, no Python or service needed at runtime.
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
| Host | macOS 15+ on Apple silicon |

`deck.py doctor` also recognises CH340, CP2102, ATmega16U2 (Uno/Nano) and Nano Every USB chips, but only the board above has been tested.

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

### 2. Install the Mac app

Download `ConsoleDeck.zip` from [Releases](https://github.com/mike623/consoledeck/releases) (or the latest [Build](https://github.com/mike623/consoledeck/actions/workflows/build.yml) run), unzip it, and move `ConsoleDeck.app` to Applications.

> [!IMPORTANT]
> CI builds are not notarized, so macOS blocks the first launch. Either run
> `xattr -dr com.apple.quarantine /Applications/ConsoleDeck.app`,
> or try to open it once and then click **Open Anyway** in System Settings → Privacy & Security.

To build it yourself (Xcode 16+):

```bash
mac/build.sh install   # builds, copies to ~/Applications and launches
```

`build.sh` signs with your Apple Development identity when it finds one, so the Accessibility permission survives rebuilds.

### 3. Calibrate and assign actions

1. Click the grid icon in the menu bar → **Calibrate Buttons…**, then press each button in the order shown.
2. Open **Edit Actions…**, pick a button and gesture, and choose an action. Use **Test** to try it.
3. Add a per-app profile with **Choose App…**; buttons left on **Use Default** fall back to the default profile.

> [!NOTE]
> **Press Keys** and **Type Text** need Accessibility access. The Actions window shows an **Allow…** button when it's missing.

## Actions

| Type | What it does |
| --- | --- |
| Open URL | Opens a link in the default browser |
| Open App | Launches or focuses an app |
| Run Script | Runs a bash command or script path |
| Press Keys | Sends a recorded key combo, e.g. `⌘⇧4` |
| Repeat Keys | Toggles pressing a key combo every N seconds; stop it from the menu |
| Type Text | Types a string, newlines become Return |
| Input Language | Switches to a specific input source |
| System | Lock Screen, Sleep, Turn Display Off, Screen Saver, Next Input Language, Mute, Volume Up/Down, Log Out, Restart, Shut Down, Open ConsoleDeck Editor |

Log Out, Restart and Shut Down go through macOS's own confirmation dialog, so a stray press can't lose work.

## Configuration

Settings live in `~/Library/Application Support/ConsoleDeck/deck.json` (menu → **Show Config in Finder**). The app re-reads it on every press, so hand edits apply immediately. Timing knobs without UI (defaults shown):

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

A terminal alternative to the app, sharing the same config file. Needs [uv](https://docs.astral.sh/uv/).

```bash
uv run deck.py                            # interactive menu: calibrate, assign actions
uv run deck.py doctor                     # check USB chip, firmware ping, config, service
uv run deck.py service install|uninstall|status   # run as a LaunchAgent instead of the app
uv run python test_deck.py                # tests
```

Set `DECK_PORT` to override the auto-detected `/dev/cu.usbserial*` / `/dev/cu.usbmodem*` port. The CLI supports the default profile with URL, app and script actions only.

## Project layout

```
deck_debug/   Arduino firmware (pin finder + health ping)
mac/          Swift menu bar app, tests and build.sh
deck.py       terminal controller, doctor and LaunchAgent service
```
