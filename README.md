# consoledeck

Terminal controller for a [Console Deck V2](https://makerworld.com/en/models/1717141-console-deck-v2) on macOS, wired however you like.

- `deck_debug/deck_debug.ino` — Arduino Nano firmware. Prints `D7 PRESSED` for any pin, answers `?` with `PONG consoledeck`. 115200 baud.
- `deck.py` — menu to calibrate buttons (3-3-4 layout), assign actions (URL / app / bash script), install the LaunchAgent service, and run `doctor`.

```
uv run deck.py            # menu
uv run deck.py doctor     # check board, firmware, config, service
uv run python test_deck.py
```

Config is saved to `deck.json` (git-ignored). Stop the service (`deck.py service uninstall`) before uploading firmware; it holds the serial port.
