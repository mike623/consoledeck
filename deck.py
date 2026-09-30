#!/usr/bin/env python3
"""ConsoleDeck for macOS: calibrate buttons, assign actions, run as a service.

Usage:
  uv run deck.py                               interactive menu
  uv run deck.py run                           listen to the deck (what the service runs)
  uv run deck.py calibrate
  uv run deck.py config
  uv run deck.py doctor                        check board, firmware ping, config, service
  uv run deck.py service install|uninstall|status

Firmware: deck_debug/deck_debug.ino (prints "D7 PRESSED" at 115200 baud).
Serial port: first /dev/cu.usbserial* or /dev/cu.usbmodem*, override with DECK_PORT.
"""
import glob
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
CONFIG = HERE / "deck.json"
BAUD = 115200
LAYOUT = [[1, 2, 3], [4, 5, 6], [7, 8, 9, 10]]  # 3.3.4, numbered left-top to right-bottom
POSITIONS = [n for row in LAYOUT for n in row]
LABEL = "com.consoledeck"
PLIST = Path.home() / "Library/LaunchAgents" / f"{LABEL}.plist"
LOG = Path.home() / "Library/Logs/consoledeck.log"
PRESS = re.compile(r"^([DA]\d+) PRESSED$")
TYPES = {"u": "url", "a": "app", "s": "script", "n": "none"}
PROMPTS = {"url": "  URL: ", "app": "  App name (e.g. Safari): ", "script": "  Bash command or script path: "}

PLIST_XML = """<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key><array>
    <string>{python}</string><string>{script}</string><string>run</string>
  </array>
  <key>EnvironmentVariables</key><dict>
    <key>PATH</key><string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict></plist>
"""


def load():
    try:
        return json.loads(CONFIG.read_text())
    except FileNotFoundError:
        return {"pins": {}, "buttons": {}}


def save(cfg):
    CONFIG.write_text(json.dumps(cfg, indent=2) + "\n")


def parse_press(line):
    m = PRESS.match(line.strip())
    return m.group(1) if m else None


def grid(mark=str):
    return "\n".join("  " + " ".join(f"[{mark(n):^4}]" for n in row) for row in LAYOUT)


def find_port():
    found = glob.glob("/dev/cu.usbserial*") + glob.glob("/dev/cu.usbmodem*")
    return os.environ.get("DECK_PORT") or (found[0] if found else None)


def open_port():
    import serial  # lazy: the menu and service commands work without pyserial

    port = find_port()
    if not port:
        raise SystemExit("No Arduino found (/dev/cu.usbserial*). Plug it in or set DECK_PORT.")
    try:
        # ponytail: exclusive so calibrate and the service can't both read the port
        return serial.Serial(port, BAUD, timeout=1, exclusive=True)
    except serial.SerialException as e:
        raise SystemExit(f"Cannot open {port}: {e}\nIf the service is running, uninstall it first.")


def presses(ser):
    while True:
        pin = parse_press(ser.readline().decode(errors="ignore"))
        if pin:
            yield pin


def run_action(action):
    kind, value = action.get("type", "none"), action.get("value", "")
    if not value:
        return
    cmd = {
        "url": ["open", value],
        "app": ["open", "-a", value],
        "script": ["/bin/bash", "-c", value],
    }.get(kind)
    if cmd:
        subprocess.Popen(cmd, cwd=Path.home())


def log(msg):
    print(f"{time.strftime('%Y-%m-%d %H:%M:%S')} {msg}", flush=True)


def run():
    import serial

    waiting = False  # log "waiting" once per disconnect, not every retry
    while True:
        try:
            ser = open_port()
        except SystemExit as e:
            if not waiting:
                log(f"{str(e).splitlines()[0]} Waiting...")
                waiting = True
            time.sleep(3)
            continue
        waiting = False
        log(f"Listening on {ser.port}")
        try:
            with ser:
                for pin in presses(ser):
                    try:
                        cfg = load()  # re-read each press so config edits apply without restarting
                    except json.JSONDecodeError as e:
                        log(f"{pin} ignored: {CONFIG.name} is not valid JSON ({e})")
                        continue
                    n = cfg["pins"].get(pin)
                    action = cfg["buttons"].get(str(n), {})
                    log(f"{pin} -> button {n}: {describe(action)}")
                    if n:
                        run_action(action)
        except serial.SerialException as e:
            log(f"Lost {ser.port} ({e}). Reconnecting...")
            time.sleep(3)


def calibrate():
    pins = {}
    print("Press each button when asked. Don't touch the knob. Ctrl-C cancels without saving.")
    with open_port() as ser:
        it = presses(ser)
        for n in POSITIONS:
            done = set(pins.values())
            print("\n" + grid(lambda k: f">{k}<" if k == n else ("ok" if k in done else str(k))))
            print(f"Press button {n}...")
            for pin in it:
                if pin in pins:
                    print(f"  {pin} is already button {pins[pin]}, press button {n}")
                    continue
                pins[pin] = n
                print(f"  button {n} = {pin}")
                break
    cfg = load()
    cfg["pins"] = pins
    save(cfg)
    print(f"\nSaved to {CONFIG.name}.")


def describe(action):
    kind = action.get("type", "none")
    return "-" if kind == "none" else f"{kind:6} {action.get('value', '')}"


def configure():
    while True:
        cfg = load()
        buttons = cfg["buttons"]
        print("\n" + grid())
        for n in POSITIONS:
            print(f"  {n:>2}  {describe(buttons.get(str(n), {}))}")
        choice = input("Button to edit (1-10), t<N> to test, Enter = back: ").strip().lower()
        if not choice:
            return
        if choice.startswith("t"):
            run_action(buttons.get(choice[1:], {}))
            continue
        if choice not in map(str, POSITIONS):
            continue
        kind = TYPES.get(input("  [u]rl  [a]pp  [s]cript  [n]one: ").strip().lower()[:1])
        if not kind:
            continue
        value = "" if kind == "none" else input(PROMPTS[kind]).strip()
        if kind == "url" and value and "://" not in value:
            value = "https://" + value
        buttons[choice] = {"type": kind, "value": value}
        save(cfg)


def launchctl(*args):
    return subprocess.run(["launchctl", *args], capture_output=True, text=True)


def service(cmd):
    domain = f"gui/{os.getuid()}"
    if cmd == "install":
        PLIST.parent.mkdir(parents=True, exist_ok=True)
        LOG.parent.mkdir(parents=True, exist_ok=True)
        PLIST.write_text(PLIST_XML.format(label=LABEL, python=sys.executable, script=HERE / "deck.py", log=LOG))
        launchctl("bootout", f"{domain}/{LABEL}")  # replace an old copy if present
        r = launchctl("bootstrap", domain, str(PLIST))
        print(r.stderr.strip() if r.returncode else f"Installed; also starts at login. Log: {LOG}")
    elif cmd == "uninstall":
        launchctl("bootout", f"{domain}/{LABEL}")
        PLIST.unlink(missing_ok=True)
        print("Uninstalled.")
    elif cmd == "status":
        r = launchctl("print", f"{domain}/{LABEL}")
        if r.returncode:
            print("Not running" + ("" if PLIST.exists() else " (not installed)"))
        else:
            state = re.search(r"\n\s*state = (.+)", r.stdout)
            pid = re.search(r"\n\s*pid = (\d+)", r.stdout)
            print(f"state: {state.group(1) if state else '?'}   pid: {pid.group(1) if pid else '-'}")
        if LOG.exists():
            # ponytail: log is never rotated; truncate it by hand if it ever gets big
            print("--- last log lines ---\n" + "\n".join(LOG.read_text().splitlines()[-10:]))
    else:
        print("service install|uninstall|status")


# USB-serial chips found on Nano boards and clones, by (vid, pid)
NANO_CHIPS = {
    (0x0403, 0x6001): "FTDI FT232R (classic Nano)",
    (0x1A86, 0x7523): "CH340 (Nano clone)",
    (0x10C4, 0xEA60): "CP2102 (Nano clone)",
    (0x2341, 0x0043): "Arduino Uno/Nano (ATmega16U2)",
    (0x2341, 0x0058): "Arduino Nano Every",
}


def ping(ser, wait=3.0):
    """Send '?' until the firmware answers. Returns 'pong', 'old' (pin finder without ping), 'garbage' or 'silent'."""
    seen = ""
    end = time.time() + wait
    while time.time() < end:
        ser.write(b"?")
        line = ser.readline().decode(errors="replace").strip()
        if "PONG consoledeck" in line:
            return "pong"
        seen += line
    if "Pin finder" in seen or "PRESSED" in seen or "released" in seen:
        return "old"
    return "garbage" if seen else "silent"


def doctor():
    ok = lambda msg: print(f"  [ok]   {msg}")
    bad = lambda msg: print(f"  [FAIL] {msg}")
    warn = lambda msg: print(f"  [warn] {msg}")
    print("ConsoleDeck doctor")

    try:
        import serial
        from serial.tools import list_ports
    except ImportError:
        return bad("pyserial missing: run `uv sync` in this folder")

    usb = [p for p in list_ports.comports() if p.vid]
    for p in usb:
        chip = NANO_CHIPS.get((p.vid, p.pid))
        (ok if chip else warn)(f"{p.device}  {p.vid:04x}:{p.pid:04x}  {chip or p.description or 'unknown chip'}")
    if not any((p.vid, p.pid) in NANO_CHIPS for p in usb):
        bad("no Nano-compatible USB board found: check cable (must carry data) and board LED")

    port = find_port()
    if not port:
        return bad("no /dev/cu.usbserial* or /dev/cu.usbmodem* port (set DECK_PORT to override)")
    ok(f"using port {port}")

    try:
        ser = serial.Serial(port, BAUD, timeout=0.5, exclusive=True)
    except serial.SerialException:
        holder = subprocess.run(["lsof", "-t", port], capture_output=True, text=True).stdout.split()
        pid = launchctl("print", f"gui/{os.getuid()}/{LABEL}").stdout
        if holder and f"pid = {holder[0]}" in pid:
            ok(f"port is in use by the running service (pid {holder[0]}), so no ping; see `service status`")
        else:
            bad(f"port busy (pid {', '.join(holder) or '?'}): close Arduino IDE Serial Monitor")
        ser = None
    if ser:
        with ser:
            time.sleep(2)  # opening the port resets the Nano; wait for boot
            result = ping(ser)
        {
            "pong": lambda: ok("firmware answered ping"),
            "old": lambda: warn("older firmware without ping: re-upload deck_debug.ino"),
            "garbage": lambda: bad(f"unreadable data: wrong firmware or baud (expect {BAUD})"),
            "silent": lambda: bad("board silent: upload deck_debug.ino (tap RESET during upload)"),
        }[result]()

    cfg = load()
    n_pins = len(cfg["pins"])
    (ok if n_pins == len(POSITIONS) else warn)(f"calibrated {n_pins}/{len(POSITIONS)} buttons")
    n_actions = sum(1 for a in cfg["buttons"].values() if a.get("type", "none") != "none")
    (ok if n_actions else warn)(f"{n_actions} buttons have actions")
    (ok if PLIST.exists() else warn)("service installed" if PLIST.exists() else "service not installed")


def menu():
    items = {
        "1": ("Calibrate buttons", calibrate),
        "2": ("Assign actions", configure),
        "3": ("Run here to test (Ctrl-C stops)", run),
        "4": ("Install service", lambda: service("install")),
        "5": ("Uninstall service", lambda: service("uninstall")),
        "6": ("Service status", lambda: service("status")),
        "7": ("Doctor (check board, firmware, config)", doctor),
    }
    while True:
        print("\nConsoleDeck")
        for key, (label, _) in items.items():
            print(f" {key}  {label}")
        print(" q  Quit")
        try:
            choice = input("> ").strip().lower()
            if choice == "q":
                return
            if choice in items:
                items[choice][1]()
        except KeyboardInterrupt:
            print("\n(cancelled)")
        except SystemExit as e:
            print(e)
        except EOFError:
            return


if __name__ == "__main__":
    args = sys.argv[1:]
    commands = {"run": run, "calibrate": calibrate, "config": configure, "doctor": doctor}
    if not args:
        menu()
    elif args[0] == "service" and len(args) == 2:
        service(args[1])
    elif args[0] in commands:
        commands[args[0]]()
    else:
        print(__doc__)
