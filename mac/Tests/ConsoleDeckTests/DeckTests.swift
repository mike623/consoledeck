import Foundation
import Testing
@testable import ConsoleDeck

@Test func parsesButtonEvents() {
    #expect(parseButton("D7 PRESSED\r\n")! == ("D7", true))
    #expect(parseButton("A0 PRESSED")! == ("A0", true))
    #expect(parseButton("D7 released")! == ("D7", false))
    #expect(parseButton("PONG consoledeck") == nil)
    #expect(parseButton("D13 is LOW at boot (held, shorted to GND, or encoder resting)") == nil)
}

@Test func readsDeckPyConfig() throws {
    let json = #"{"pins": {"D6": 1, "D5": 5}, "buttons": {"1": {"type": "app", "value": "Spotify"}}}"#
    let config = try JSONDecoder().decode(Config.self, from: Data(json.utf8))
    #expect(config.action(forPin: "D6")?.button == 1)
    #expect(config.action(forPin: "D6")?.action == Action(type: "app", value: "Spotify"))
    #expect(config.action(forPin: "D5")?.action.type == "none")
    #expect(config.action(forPin: "D2") == nil)
}

@Test func configRoundTrips() throws {
    let config = Config(pins: ["D6": 1, "A2": 8], buttons: ["1": Action(type: "url", value: "https://x.com")])
    let decoded = try JSONDecoder().decode(Config.self, from: JSONEncoder().encode(config))
    #expect(decoded == config)
    #expect(config.pin(forButton: 8) == "A2")
    #expect(config.pin(forButton: 2) == nil)
}

@Test func frontAppProfileOverridesDefault() {
    let config = Config(
        pins: ["D6": 1, "D7": 2, "D8": 3],
        buttons: ["1": Action(type: "app", value: "Spotify"), "2": Action(type: "app", value: "Finder")],
        profiles: ["com.apple.Safari": Profile(name: "Safari", buttons: [
            "1": Action(type: "url", value: "https://x.com"),
            "2": Action(type: "none"),  // explicitly off in Safari
        ])]
    )
    let safari = "com.apple.Safari"
    #expect(config.action(forPin: "D6", app: safari)?.action.value == "https://x.com")
    #expect(config.action(forPin: "D6", app: safari)?.profile == "Safari")
    #expect(config.action(forPin: "D7", app: safari)?.action.type == "none")
    #expect(config.action(forPin: "D8", app: safari)?.profile == nil)  // not in profile: default
    #expect(config.action(forPin: "D6", app: "com.other")?.action.value == "Spotify")
    #expect(config.action(forPin: "D6")?.profile == nil)
}

@Test func oldConfigWithoutProfilesStillLoads() throws {
    let config = try JSONDecoder().decode(Config.self, from: Data(#"{"pins": {"D6": 1}}"#.utf8))
    #expect(config.profiles.isEmpty && config.buttons.isEmpty && config.pins["D6"] == 1)
}

@Test func ownWindowsKeepPreviousFrontApp() {
    let safari = FrontApp(id: "com.apple.Safari", name: "Safari")
    let own = FrontApp(id: "com.consoledeck.app", name: "ConsoleDeck")
    #expect(frontApp(afterActivating: own, ownID: own.id, previous: safari) == safari)
    #expect(frontApp(afterActivating: safari, ownID: own.id, previous: nil) == safari)
    #expect(frontApp(afterActivating: nil, ownID: own.id, previous: safari) == safari)
}

@Test func editingProfileButtons() {
    var config = Config(pins: ["D6": 1], buttons: ["1": Action(type: "app", value: "Spotify")],
                        profiles: ["com.apple.Safari": Profile(name: "Safari")])
    let safari = "com.apple.Safari"
    #expect(config.ownAction(button: 1, profile: safari) == nil)  // inherits
    config.setAction(Action(type: "url", value: "x.com"), button: 1, profile: safari)
    #expect(config.action(forPin: "D6", app: safari)?.action.value == "x.com")
    #expect(config.buttons["1"]?.value == "Spotify")  // default untouched
    config.setAction(nil, button: 1, profile: safari)
    #expect(config.action(forPin: "D6", app: safari)?.action.value == "Spotify")
    config.setAction(Action(type: "url", value: "y.com"), button: 1, profile: "com.gone")  // deleted profile: no-op
    #expect(config.profiles["com.gone"] == nil)
}

@Test func keyCombos() {
    #expect(KeyCombo("space") == KeyCombo("SPACE"))
    #expect(KeyCombo("space")?.key == 49)
    #expect(KeyCombo("cmd+shift+t")?.key == 17)
    #expect(KeyCombo("cmd+shift+t")?.flags == [.maskCommand, .maskShift])
    #expect(KeyCombo("kc105")?.key == 105)
    #expect(KeyCombo("hyper+t") == nil)
    #expect(KeyCombo("") == nil)
    #expect(keyComboText(keyCode: 17, modifierFlags: [.command, .shift, .capsLock]) == "shift+cmd+t")
    #expect(keyComboText(keyCode: 49, modifierFlags: []) == "space")
    #expect(keyComboText(keyCode: 105, modifierFlags: []) == "kc105")
    #expect(keyComboSymbols("shift+cmd+t") == "⇧⌘T")
    #expect(keyComboSymbols("space") == "Space")
    // what gets recorded must parse back to the same key
    let recorded = keyComboText(keyCode: 36, modifierFlags: [.option])
    #expect(recorded == "opt+return" && KeyCombo(recorded)?.key == 36 && KeyCombo(recorded)?.flags == .maskAlternate)
}

@Test func typingSplitsLongTextAndNewlines() {
    #expect(typingSteps("hi") == [.text(Array("hi".utf16))])
    #expect(typingSteps("a\nb") == [.text([97]), .returnKey, .text([98])])
    #expect(typingSteps("\n") == [.returnKey])
    let long = String(repeating: "x", count: 45)
    #expect(typingSteps(long).map { if case .text(let u) = $0 { u.count } else { -1 } } == [20, 20, 5])
    // a 2-unit emoji at the boundary moves to the next event instead of being split
    let steps = typingSteps(String(repeating: "x", count: 19) + "😀")
    #expect(steps.count == 2 && steps[1] == .text(Array("😀".utf16)))
}

@Test func gestureKeysInheritLikePresses() {
    var config = Config(pins: ["D6": 1], buttons: ["1": Action(type: "app", value: "Spotify"),
                                                   "1.long": Action(type: "app", value: "Music")],
                        profiles: ["com.apple.Safari": Profile(name: "Safari")])
    #expect(config.action(button: 1, gesture: .long, app: "com.apple.Safari").action.value == "Music")
    config.setAction(Action(type: "none"), button: 1, gesture: .long, profile: "com.apple.Safari")
    #expect(config.action(button: 1, gesture: .long, app: "com.apple.Safari").action.type == "none")
    #expect(config.action(button: 1, gesture: .press, app: "com.apple.Safari").action.value == "Spotify")
    #expect(config.action(button: 1, gesture: .double, app: nil).action.type == "none")
}

/// Manual clock so gesture timing is tested without sleeping.
@MainActor final class FakeClock {
    var now = 0.0
    var timers: [(at: Double, fn: @MainActor () -> Void, live: Bool)] = []

    func schedule(_ delay: TimeInterval, _ fn: @escaping @MainActor () -> Void) -> () -> Void {
        let id = timers.count
        timers.append((now + delay, fn, true))
        return { self.timers[id].live = false }
    }

    func advance(_ seconds: Double) {
        now += seconds
        for i in timers.indices where timers[i].live && timers[i].at <= now {
            timers[i].live = false
            timers[i].fn()
        }
    }
}

@MainActor @Test func gestures() {
    let clock = FakeClock()
    var fired: [Gesture] = []
    let g = GestureDetector(schedule: clock.schedule) { _, gesture in fired.append(gesture) }

    // no long/double actions: fires on the way down, no waiting
    g.down(1, uses: []); #expect(fired == [.press]); g.up(1)
    clock.advance(1); #expect(fired == [.press])

    // long press: fires while still held; release does nothing more
    fired = []
    g.down(1, uses: [.long]); clock.advance(0.6); #expect(fired == [.long])
    g.up(1); clock.advance(1); #expect(fired == [.long])

    // short tap on a button with a long action: press on release
    fired = []
    g.down(1, uses: [.long]); clock.advance(0.2); g.up(1); #expect(fired == [.press])
    clock.advance(1); #expect(fired == [.press])

    // double press: second down within the gap
    fired = []
    g.down(1, uses: [.double]); g.up(1); clock.advance(0.1)
    g.down(1, uses: [.double]); #expect(fired == [.double])
    g.up(1); clock.advance(1); #expect(fired == [.double])

    // single tap on a button with a double action: press after the gap
    fired = []
    g.down(1, uses: [.double, .long]); g.up(1); #expect(fired == [])
    clock.advance(0.4); #expect(fired == [.press])

    // buttons are independent
    fired = []
    g.down(1, uses: [.long]); g.down(2, uses: []); #expect(fired == [.press])
    clock.advance(0.6); #expect(fired == [.press, .long])
    g.up(1); g.up(2)

    // release we never saw the press for (port opened mid-hold) is ignored
    fired = []
    g.up(3); clock.advance(1); #expect(fired == [])
}

@Test func systemActionsAllRunnable() {
    for (id, _) in systemActions where id != "lock" {  // lock is a key press, not a command
        #expect(systemCommand(id) != nil, "no command for \(id)")
    }
    #expect(KeyCombo("ctrl+cmd+q") != nil)
    #expect(systemCommand("lock") == nil && systemCommand("selfdestruct") == nil)
    #expect(describe(Action(type: "system", value: "restart")) == "Restart…")
}
