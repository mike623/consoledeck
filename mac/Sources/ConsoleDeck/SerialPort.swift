import Foundation

struct SerialError: Error, CustomStringConvertible {
    let description: String
}

/// First /dev/cu.usbserial* then /dev/cu.usbmodem*, same as deck.py. DECK_PORT overrides.
func findPort() -> String? {
    if let port = ProcessInfo.processInfo.environment["DECK_PORT"] { return port }
    let names = ((try? FileManager.default.contentsOfDirectory(atPath: "/dev")) ?? []).sorted()
    let match = names.first { $0.hasPrefix("cu.usbserial") } ?? names.first { $0.hasPrefix("cu.usbmodem") }
    return match.map { "/dev/" + $0 }
}

/// Raw 115200 8N1 serial port. Locked with flock, like pyserial's exclusive=True,
/// so deck.py calibrate and this app can't both read it.
final class SerialPort {
    let path: String
    private let fd: Int32

    init(path: String) throws {
        self.path = path
        fd = open(path, O_RDWR | O_NOCTTY | O_NONBLOCK)
        guard fd >= 0 else { throw SerialError(description: "Cannot open \(path): \(String(cString: strerror(errno)))") }
        guard flock(fd, LOCK_EX | LOCK_NB) == 0 else {
            close(fd)
            throw SerialError(description: "\(path) is busy (deck.py or Arduino IDE has it)")
        }
        var t = termios()
        tcgetattr(fd, &t)
        cfmakeraw(&t)
        cfsetspeed(&t, speed_t(B115200))
        t.c_cflag |= tcflag_t(CLOCAL | CREAD)
        withUnsafeMutableBytes(of: &t.c_cc) { cc in
            cc[Int(VMIN)] = 0
            cc[Int(VTIME)] = 10  // read() returns after 1s of silence so unplug is noticed
        }
        tcsetattr(fd, TCSANOW, &t)
        _ = fcntl(fd, F_SETFL, 0)  // back to blocking reads
        tcflush(fd, TCIFLUSH)
    }

    deinit { close(fd) }

    /// Calls onLine for each line until the board disappears, then throws.
    func readLines(_ onLine: (String) -> Void) throws {
        var buf = [UInt8](repeating: 0, count: 256)
        var line: [UInt8] = []
        // Opening the port reboots the Nano; bytes arriving before it's up are stale
        // (queued in the USB chip) and would replay old presses.
        let settled = Date().addingTimeInterval(2)
        while true {
            let n = read(fd, &buf, buf.count)
            if n < 0 {
                if errno == EINTR { continue }
                throw SerialError(description: "Lost \(path): \(String(cString: strerror(errno)))")
            }
            if n == 0 {
                if !FileManager.default.fileExists(atPath: path) { throw SerialError(description: "Deck unplugged") }
                continue
            }
            if Date() < settled { continue }
            for byte in buf[..<n] {
                if byte == UInt8(ascii: "\n") {
                    onLine(String(decoding: line, as: UTF8.self))
                    line.removeAll(keepingCapacity: true)
                } else if line.count < 256 {  // garbage without newlines can't grow forever
                    line.append(byte)
                }
            }
        }
    }
}
