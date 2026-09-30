from deck import grid, parse_press

assert parse_press("D7 PRESSED\r\n") == "D7"
assert parse_press("A0 PRESSED") == "A0"
assert parse_press("D7 released") is None
assert parse_press("D13 is LOW at boot (held, shorted to GND, or encoder resting)") is None
assert grid().count("[") == 10
assert grid().splitlines()[2].count("[") == 4
print("ok")

class FakeSerial:
    def __init__(self, lines): self.lines = list(lines)
    def write(self, _): pass
    def readline(self): return self.lines.pop(0) if self.lines else b""

from deck import ping
assert ping(FakeSerial([b"=== Pin finder ===\r\n", b"PONG consoledeck\r\n"])) == "pong"
assert ping(FakeSerial([b"D7 PRESSED\r\n"]), wait=0.1) == "old"
assert ping(FakeSerial([b"\xef\xbf"]), wait=0.1) == "garbage"
assert ping(FakeSerial([]), wait=0.1) == "silent"
print("ping ok")
