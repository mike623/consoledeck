//! Finds the deck and reads "D7 PRESSED" / "D7 released" lines from it.

use std::io::{ErrorKind, Read};
use std::time::{Duration, Instant};

/// USB-serial chips found on Nano boards and clones (same list as deck.py doctor).
const NANO_CHIPS: [(u16, u16); 5] = [
    (0x0403, 0x6001), // FTDI FT232R (classic Nano)
    (0x1A86, 0x7523), // CH340 (Nano clone)
    (0x10C4, 0xEA60), // CP2102 (Nano clone)
    (0x2341, 0x0043), // Arduino Uno/Nano (ATmega16U2)
    (0x2341, 0x0058), // Arduino Nano Every
];

/// A USB serial port as (name, vid, pid).
pub type UsbPort = (String, u16, u16);

/// `DECK_PORT` wins; otherwise the first known Nano chip. On Mac each device shows up twice;
/// /dev/cu.* is the one to use (/dev/tty.* waits for carrier detect).
pub fn pick_port(ports: &[UsbPort], override_port: Option<String>) -> Option<String> {
    if override_port.is_some() {
        return override_port;
    }
    let mut nanos: Vec<&String> = ports.iter().filter(|(_, v, p)| NANO_CHIPS.contains(&(*v, *p))).map(|p| &p.0).collect();
    nanos.sort_by_key(|name| name.starts_with("/dev/tty."));
    nanos.first().map(|n| n.to_string())
}

pub fn usb_ports() -> Vec<UsbPort> {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| match p.port_type {
            serialport::SerialPortType::UsbPort(u) => Some((p.port_name, u.vid, u.pid)),
            _ => None,
        })
        .collect()
}

/// Unix: the device node (or a DECK_PORT symlink) goes away on unplug. Windows: COMx leaves the port list.
fn present(name: &str) -> bool {
    if cfg!(unix) {
        std::path::Path::new(name).exists()
    } else {
        serialport::available_ports().unwrap_or_default().iter().any(|p| p.port_name == name)
    }
}

/// 115200 8N1. serialport opens Unix ports exclusively (TIOCEXCL); Windows COM ports always are.
pub fn open(name: &str) -> Result<Box<dyn serialport::SerialPort>, String> {
    let port = serialport::new(name, 115_200)
        .timeout(Duration::from_secs(1)) // read returns after 1s of silence so unplug is noticed
        .open()
        .map_err(|e| format!("Cannot open {name}: {e}"))?;
    let _ = port.clear(serialport::ClearBuffer::Input);
    Ok(port)
}

/// Calls on_line for each line until the deck disappears, then errors.
pub fn read_lines(mut port: Box<dyn serialport::SerialPort>, name: &str, mut on_line: impl FnMut(&str)) -> Result<(), String> {
    // Opening the port reboots the Nano; bytes arriving before it's up are stale
    // (queued in the USB chip) and would replay old presses.
    let settled = Instant::now() + Duration::from_secs(2);
    let mut lines = Lines::default();
    let mut buf = [0u8; 256];
    loop {
        match port.read(&mut buf) {
            Ok(n) if n > 0 => {
                if Instant::now() >= settled {
                    lines.push(&buf[..n]).iter().for_each(|l| on_line(l));
                }
            }
            // 1s of silence, or 0 bytes (how some backends report a hung-up tty): still plugged in?
            Ok(_) => {
                if !present(name) {
                    return Err("Deck unplugged".into());
                }
            }
            Err(e) if e.kind() == ErrorKind::TimedOut => {
                if !present(name) {
                    return Err("Deck unplugged".into());
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(format!("Lost {name}: {e}")),
        }
    }
}

/// Splits raw bytes into lines. Garbage without newlines can't grow forever.
#[derive(Default)]
pub struct Lines {
    buf: Vec<u8>,
}

impl Lines {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        for &b in bytes {
            if b == b'\n' {
                out.push(String::from_utf8_lossy(&self.buf).into_owned());
                self.buf.clear();
            } else if self.buf.len() < 256 {
                self.buf.push(b);
            }
        }
        out
    }
}

/// "D7 PRESSED" -> ("D7", true), "D7 released" -> ("D7", false); boot messages, PONG -> None.
pub fn parse_button(line: &str) -> Option<(String, bool)> {
    let (pin, state) = line.trim().split_once(' ')?;
    let down = match state {
        "PRESSED" => true,
        "released" => false,
        _ => return None,
    };
    let digits = pin.strip_prefix('D').or_else(|| pin.strip_prefix('A'))?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| (pin.to_string(), down))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(name: &str, vid: u16, pid: u16) -> UsbPort {
        (name.into(), vid, pid)
    }

    #[test]
    fn picks_known_chip_and_prefers_cu() {
        let ports = vec![
            port("/dev/tty.usbmodem1", 0x05AC, 0x1234), // not a Nano
            port("/dev/tty.usbserial-A1", 0x0403, 0x6001),
            port("/dev/cu.usbserial-A1", 0x0403, 0x6001),
        ];
        assert_eq!(pick_port(&ports, None).as_deref(), Some("/dev/cu.usbserial-A1"));
        assert_eq!(pick_port(&[port("COM4", 0x1A86, 0x7523)], None).as_deref(), Some("COM4"));
        assert_eq!(pick_port(&[port("COM3", 0x05AC, 1)], None), None);
    }

    #[test]
    fn deck_port_overrides() {
        assert_eq!(pick_port(&[port("COM4", 0x1A86, 0x7523)], Some("COM9".into())).as_deref(), Some("COM9"));
        assert_eq!(pick_port(&[], Some("/dev/cu.foo".into())).as_deref(), Some("/dev/cu.foo"));
    }

    #[test]
    fn splits_lines_across_reads() {
        let mut l = Lines::default();
        assert!(l.push(b"D7 PRE").is_empty());
        assert_eq!(l.push(b"SSED\r\nD7 rel"), vec!["D7 PRESSED\r"]);
        assert_eq!(l.push(b"eased\n"), vec!["D7 released"]);
    }

    #[test]
    fn caps_line_length() {
        let mut l = Lines::default();
        l.push(&[b'x'; 1000]);
        assert_eq!(l.push(b"\n")[0].len(), 256);
    }

    #[test]
    fn parses_button_events() {
        assert_eq!(parse_button("D7 PRESSED\r\n"), Some(("D7".into(), true)));
        assert_eq!(parse_button("A0 PRESSED"), Some(("A0".into(), true)));
        assert_eq!(parse_button("D13 released"), Some(("D13".into(), false)));
        assert_eq!(parse_button("PONG consoledeck"), None);
        assert_eq!(parse_button("D13 is LOW at boot (held, shorted to GND, or encoder resting)"), None);
        assert_eq!(parse_button("Dx PRESSED"), None);
        assert_eq!(parse_button("D7 pressed"), None);
    }
}
