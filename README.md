# consoledeck

Terminal controller for a [Console Deck V2](https://makerworld.com/en/models/1717141-console-deck-v2) on macOS, wired however you like.

- `deck_debug/deck_debug.ino` — Arduino Nano firmware. Prints `D7 PRESSED` for any pin, answers `?` with `PONG consoledeck`. 115200 baud.
- `mac/` — native menu bar app (Swift). `mac/build.sh install` builds it into `~/Applications/ConsoleDeck.app` and launches it. Listens to the deck, runs actions, and has Calibrate and Edit Actions windows in its menu; replaces the `deck.py` service.
- `deck.py` — menu to calibrate buttons (3-3-4 layout), assign actions (URL / app / bash script), install the LaunchAgent service, and run `doctor`.

```
uv run deck.py            # menu
uv run deck.py doctor     # check board, firmware, config, service
uv run python test_deck.py
```

Config lives in `~/Library/Application Support/ConsoleDeck/deck.json`, shared by the app and `deck.py`. Quit the app (or stop the service) before calibrating or uploading firmware; it holds the serial port.
