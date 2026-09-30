import AppKit
import Foundation
import os
import ServiceManagement

let logger = Logger(subsystem: "com.consoledeck.app", category: "deck")

/// Physical 3-3-4 layout, numbered left-top to right-bottom (same as deck.py).
let layout = [[1, 2, 3], [4, 5, 6], [7, 8, 9, 10]]
let positions = layout.flatMap { $0 }

struct Action: Codable, Equatable {
    var type: String
    var value: String?
}

/// Per-app overrides. A button missing here falls back to the default buttons.
struct Profile: Codable, Equatable {
    var name: String
    var buttons: [String: Action] = [:]
}

/// The app in front, which picks the profile.
struct FrontApp: Equatable {
    var id: String
    var name: String
}

/// ConsoleDeck's own windows (Actions, Calibrate) must not switch the profile away from the app being configured.
func frontApp(afterActivating app: FrontApp?, ownID: String?, previous: FrontApp?) -> FrontApp? {
    guard let app, app.id != ownID else { return previous }
    return app
}

struct Config: Codable, Equatable {
    var pins: [String: Int] = [:]
    var buttons: [String: Action] = [:]  // the default profile; deck.py only knows this one
    var profiles: [String: Profile] = [:]  // keyed by bundle ID

    init(pins: [String: Int] = [:], buttons: [String: Action] = [:], profiles: [String: Profile] = [:]) {
        self.pins = pins
        self.buttons = buttons
        self.profiles = profiles
    }

    // Synthesized decoding would reject files written before a key existed.
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        pins = try c.decodeIfPresent([String: Int].self, forKey: .pins) ?? [:]
        buttons = try c.decodeIfPresent([String: Action].self, forKey: .buttons) ?? [:]
        profiles = try c.decodeIfPresent([String: Profile].self, forKey: .profiles) ?? [:]
    }

    static let url = FileManager.default.homeDirectoryForCurrentUser
        .appending(path: "Library/Application Support/ConsoleDeck/deck.json")

    static func load() throws -> Config {
        try JSONDecoder().decode(Config.self, from: Data(contentsOf: url))
    }

    func save() throws {
        try FileManager.default.createDirectory(at: Self.url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(self).write(to: Self.url, options: .atomic)
    }

    /// What `profile` (nil = default) itself sets for button n. nil in a profile means "use default".
    func ownAction(button n: Int, profile: String?) -> Action? {
        guard let profile else { return buttons[String(n)] }
        return profiles[profile]?.buttons[String(n)]
    }

    /// nil removes the button: back to "use default" in a profile, "none" in the default.
    mutating func setAction(_ action: Action?, button n: Int, profile: String?) {
        if let profile {
            profiles[profile]?.buttons[String(n)] = action
        } else {
            buttons[String(n)] = action
        }
    }

    func pin(forButton n: Int) -> String? {
        pins.first { $0.value == n }?.key
    }

    /// Front app's profile first, then the default buttons. `profile` is nil when the default was used.
    func action(forPin pin: String, app: String? = nil) -> (button: Int, action: Action, profile: String?)? {
        guard let n = pins[pin] else { return nil }
        if let app, let profile = profiles[app], let action = profile.buttons[String(n)] {
            return (n, action, profile.name)
        }
        return (n, buttons[String(n)] ?? Action(type: "none"), nil)
    }
}

/// "D7 PRESSED" -> "D7"; anything else (released, boot messages, PONG) -> nil.
func parsePress(_ line: String) -> String? {
    let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.wholeMatch(of: /([DA]\d+) PRESSED/).map { String($0.1) }
}

func describe(_ action: Action) -> String {
    switch action.type {
    case "none": "no action"
    case "key": "key \(keyComboSymbols(action.value ?? ""))"
    default: "\(action.type) \(action.value ?? "")"
    }
}

@MainActor
func run(_ action: Action) {
    guard let value = action.value, !value.isEmpty else { return }
    let args: [String]
    switch action.type {
    case "url":
        if let url = URL(string: value.contains("://") ? value : "https://" + value) { NSWorkspace.shared.open(url) }
        return
    case "key":
        guard let combo = KeyCombo(value) else { return logger.error("Unknown key combo \(value, privacy: .public)") }
        guard accessibilityTrusted(prompt: true) else {
            return logger.error("Key \(value, privacy: .public) not sent: ConsoleDeck needs Accessibility permission")
        }
        press(combo)
        return
    case "app": args = ["/usr/bin/open", "-a", value]
    case "script": args = ["/bin/bash", "-c", value]
    default: return
    }
    let p = Process()
    p.executableURL = URL(fileURLWithPath: args[0])
    p.arguments = Array(args.dropFirst())
    p.currentDirectoryURL = FileManager.default.homeDirectoryForCurrentUser
    // Apps launched from Finder get a bare PATH; scripts expect Homebrew tools.
    var env = ProcessInfo.processInfo.environment
    env["PATH"] = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
    p.environment = env
    do { try p.run() } catch { logger.error("\(args, privacy: .public) failed: \(error, privacy: .public)") }
}

@MainActor @Observable
final class Deck {
    var status = "Starting..."
    var lastPress = ""
    var config = Config()
    /// Non-nil while calibrating: presses are recorded as pin -> button instead of running actions.
    var calibration: [String: Int]?
    var calibrationNote = ""
    var frontApp: FrontApp?
    var startsAtLogin = SMAppService.mainApp.status == .enabled

    func setStartsAtLogin(_ on: Bool) {
        do {
            if on { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() }
        } catch {
            logger.error("Start at login \(on ? "on" : "off", privacy: .public) failed: \(error, privacy: .public)")
        }
        let status = SMAppService.mainApp.status
        if status == .requiresApproval { SMAppService.openSystemSettingsLoginItems() }  // user switched it off there before
        startsAtLogin = status == .enabled
    }

    var profileName: String {
        frontApp.flatMap { config.profiles[$0.id]?.name } ?? "Default"
    }

    init() {
        reload()
        let own = Bundle.main.bundleIdentifier
        let current = NSWorkspace.shared.frontmostApplication
        frontApp = ConsoleDeck.frontApp(afterActivating: current.flatMap(FrontApp.init), ownID: own, previous: nil)
        NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main
        ) { [weak self] note in
            let app = (note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication).flatMap(FrontApp.init)
            MainActor.assumeIsolated {
                guard let self else { return }
                let next = ConsoleDeck.frontApp(afterActivating: app, ownID: own, previous: self.frontApp)
                guard next != self.frontApp else { return }
                self.frontApp = next
                logger.notice("Front app \(next?.id ?? "-", privacy: .public), profile \(self.profileName, privacy: .public)")
            }
        }
        Thread.detachNewThread { [self] in listenForever() }
    }

    /// Picks up edits made by deck.py or by hand; keeps the last good config if the file is broken.
    func reload() {
        do { config = try Config.load() } catch CocoaError.fileReadNoSuchFile {
            config = Config()
        } catch {
            logger.error("Cannot read deck.json: \(error, privacy: .public)")
        }
    }

    func save() {
        do { try config.save() } catch { logger.error("Cannot save deck.json: \(error, privacy: .public)") }
    }

    var nextToCalibrate: Int? {
        calibration.flatMap { cal in positions.first { !cal.values.contains($0) } }
    }

    func startCalibration() {
        calibration = [:]
        calibrationNote = "Press button 1"
    }

    func cancelCalibration() {
        if calibration != nil { calibrationNote = "Cancelled, nothing changed" }
        calibration = nil
    }

    private func calibrate(_ pin: String) {
        guard let n = nextToCalibrate else { return }
        if let existing = calibration?[pin] {
            calibrationNote = "\(pin) is already button \(existing). Press button \(n)"
            return
        }
        calibration?[pin] = n
        if let next = nextToCalibrate {
            calibrationNote = "Button \(n) = \(pin). Press button \(next)"
        } else {
            reload()
            config.pins = calibration ?? [:]
            save()
            calibration = nil
            calibrationNote = "Saved all \(positions.count) buttons"
        }
    }

    nonisolated private func listenForever() {
        var waiting = false  // report "waiting" once per disconnect, not every retry
        while true {
            guard let path = findPort() else {
                if !waiting { report("Waiting for deck (plug in USB)") }
                waiting = true
                Thread.sleep(forTimeInterval: 3)
                continue
            }
            do {
                let port = try SerialPort(path: path)
                waiting = false
                report("Listening on \(path)")
                try port.readLines { line in
                    if let pin = parsePress(line) { Task { @MainActor in self.pressed(pin) } }
                }
            } catch {
                if !waiting { report("\(error)") }
                waiting = true
                Thread.sleep(forTimeInterval: 3)
            }
        }
    }

    nonisolated private func report(_ msg: String) {
        logger.notice("\(msg, privacy: .public)")
        Task { @MainActor in self.status = msg }
    }

    private func pressed(_ pin: String) {
        if calibration != nil { return calibrate(pin) }
        reload()  // re-read each press so deck.py or hand edits apply immediately
        guard let (n, action, profile) = config.action(forPin: pin, app: frontApp?.id) else {
            lastPress = "\(pin): not calibrated"
            return
        }
        let summary = "button \(n) → \(profile.map { "\($0): " } ?? "")\(describe(action))"
        lastPress = "Last: " + summary
        logger.notice("\(pin, privacy: .public) -> \(summary, privacy: .public)")
        run(action)
    }
}

extension FrontApp {
    init?(_ app: NSRunningApplication) {
        guard let id = app.bundleIdentifier else { return nil }
        self.init(id: id, name: app.localizedName ?? id)
    }
}
