import AppKit
import ApplicationServices

/// Key names <-> virtual key codes (physical positions, ANSI/ISO). Values are stored as
/// readable combos like "space" or "cmd+shift+t"; keys not listed are stored as "kc<code>".
let keyCodes: [String: CGKeyCode] = [
    "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "§": 10,
    "b": 11, "q": 12, "w": 13, "e": 14, "r": 15, "y": 16, "t": 17, "1": 18, "2": 19, "3": 20,
    "4": 21, "6": 22, "5": 23, "=": 24, "9": 25, "7": 26, "-": 27, "8": 28, "0": 29, "]": 30,
    "o": 31, "u": 32, "[": 33, "i": 34, "p": 35, "return": 36, "l": 37, "j": 38, "'": 39,
    "k": 40, ";": 41, "\\": 42, ",": 43, "/": 44, "n": 45, "m": 46, ".": 47, "tab": 48,
    "space": 49, "`": 50, "delete": 51, "escape": 53,
    "f1": 122, "f2": 120, "f3": 99, "f4": 118, "f5": 96, "f6": 97, "f7": 98, "f8": 100,
    "f9": 101, "f10": 109, "f11": 103, "f12": 111,
    "home": 115, "pageup": 116, "forwarddelete": 117, "end": 119, "pagedown": 121,
    "left": 123, "right": 124, "down": 125, "up": 126,
]
private let keyNames = Dictionary(uniqueKeysWithValues: keyCodes.map { ($1, $0) })

/// Apple's display order: ⌃⌥⇧⌘.
private let modifiers: [(name: String, symbol: String, flag: CGEventFlags, nsFlag: NSEvent.ModifierFlags)] = [
    ("ctrl", "⌃", .maskControl, .control),
    ("opt", "⌥", .maskAlternate, .option),
    ("shift", "⇧", .maskShift, .shift),
    ("cmd", "⌘", .maskCommand, .command),
]

private let symbols: [String: String] = [
    "space": "Space", "return": "↩", "tab": "⇥", "delete": "⌫", "forwarddelete": "⌦", "escape": "⎋",
    "left": "←", "right": "→", "up": "↑", "down": "↓", "home": "↖", "end": "↘", "pageup": "⇞", "pagedown": "⇟",
]

struct KeyCombo: Equatable {
    var key: CGKeyCode
    var flags: CGEventFlags

    /// "cmd+shift+t", "space", "kc105" -> KeyCombo. nil if any part is unknown.
    init?(_ text: String) {
        var parts = text.lowercased().split(separator: "+").map { $0.trimmingCharacters(in: .whitespaces) }
        guard let last = parts.popLast(), !last.isEmpty else { return nil }
        if let code = keyCodes[last] {
            key = code
        } else if last.hasPrefix("kc"), let code = CGKeyCode(last.dropFirst(2)) {
            key = code
        } else {
            return nil
        }
        flags = []
        for part in parts {
            guard let m = modifiers.first(where: { $0.name == part }) else { return nil }
            flags.insert(m.flag)
        }
    }
}

/// Recorded key event -> stored text, e.g. keyCode 17 + ⌘⇧ -> "cmd+shift+t".
func keyComboText(keyCode: UInt16, modifierFlags: NSEvent.ModifierFlags) -> String {
    let mods = modifiers.filter { modifierFlags.contains($0.nsFlag) }.map(\.name)
    return (mods + [keyNames[keyCode] ?? "kc\(keyCode)"]).joined(separator: "+")
}

/// "cmd+shift+t" -> "⌘⇧T" for the UI.
func keyComboSymbols(_ text: String) -> String {
    let parts = text.lowercased().split(separator: "+").map(String.init)
    guard let key = parts.last else { return text }
    let mods = modifiers.filter { parts.dropLast().contains($0.name) }.map(\.symbol).joined()
    return mods + (symbols[key] ?? key.uppercased())
}

/// Posts the combo to whatever app is in front. Needs Accessibility permission.
func press(_ combo: KeyCombo) {
    let source = CGEventSource(stateID: .hidSystemState)
    for down in [true, false] {
        let event = CGEvent(keyboardEventSource: source, virtualKey: combo.key, keyDown: down)
        event?.flags = combo.flags
        event?.post(tap: .cghidEventTap)
    }
}

/// With prompt, macOS shows its "allow ConsoleDeck to control this computer" dialog if not yet trusted.
func accessibilityTrusted(prompt: Bool = false) -> Bool {
    AXIsProcessTrustedWithOptions(["AXTrustedCheckOptionPrompt": prompt] as CFDictionary)
}

func openAccessibilitySettings() {
    NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!)
}

enum TypingStep: Equatable {
    case text([UniChar])
    case returnKey
}

/// Splits text into events macOS will accept: at most 20 UTF-16 units per event (longer strings
/// are cut off), and newlines as real Return presses since many apps ignore a typed "\n".
func typingSteps(_ text: String) -> [TypingStep] {
    var steps: [TypingStep] = []
    for (i, line) in text.split(separator: "\n", omittingEmptySubsequences: false).enumerated() {
        if i > 0 { steps.append(.returnKey) }
        var chunk: [UniChar] = []
        for ch in line {  // whole characters, so emoji and accents aren't split across events
            let units = Array(String(ch).utf16)
            if chunk.count + units.count > 20 {
                steps.append(.text(chunk))
                chunk = []
            }
            chunk += units
        }
        if !chunk.isEmpty { steps.append(.text(chunk)) }
    }
    return steps
}

/// Types text into the app in front, as if from a keyboard. Needs Accessibility permission.
func type(_ text: String) {
    let steps = typingSteps(text)
    DispatchQueue.global(qos: .userInitiated).async {
        let source = CGEventSource(stateID: .hidSystemState)
        for step in steps {
            for down in [true, false] {
                let event: CGEvent?
                switch step {
                case .text(var units):
                    event = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: down)
                    event?.keyboardSetUnicodeString(stringLength: units.count, unicodeString: &units)
                case .returnKey:
                    event = CGEvent(keyboardEventSource: source, virtualKey: keyCodes["return"]!, keyDown: down)
                }
                event?.flags = []  // a held modifier must not turn "a" into ⌘A
                event?.post(tap: .cghidEventTap)
            }
            usleep(5_000)  // some apps drop events that arrive in one burst
        }
    }
}
