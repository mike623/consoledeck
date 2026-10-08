import Carbon
import Foundation

/// System actions: stored value, menu label. Order is the order shown in the editor.
let systemActions: [(id: String, label: String)] = [
    ("lock", "Lock Screen"),
    ("sleep", "Sleep"),
    ("displaysleep", "Turn Display Off"),
    ("screensaver", "Start Screen Saver"),
    ("inputsource", "Next Input Language"),
    ("mute", "Mute / Unmute"),
    ("volumeup", "Volume Up"),
    ("volumedown", "Volume Down"),
    ("logout", "Log Out…"),
    ("restart", "Restart…"),
    ("shutdown", "Shut Down…"),
]

/// Command for a system action, or nil for in-process ones ("lock", "inputsource") and unknown ids.
/// Log out / restart / shut down go through loginwindow, which shows macOS's own
/// "Are you sure?" dialog, so a stray press can't lose work.
func systemCommand(_ id: String) -> [String]? {
    let osa = { (script: String) in ["/usr/bin/osascript", "-e", script] }
    return switch id {
    case "sleep": ["/usr/bin/pmset", "sleepnow"]
    case "displaysleep": ["/usr/bin/pmset", "displaysleepnow"]
    case "screensaver": ["/usr/bin/open", "-a", "ScreenSaverEngine"]
    case "mute": osa("set volume output muted (not output muted of (get volume settings))")
    case "volumeup": osa("set volume output volume ((output volume of (get volume settings)) + 6.25)")
    case "volumedown": osa("set volume output volume ((output volume of (get volume settings)) - 6.25)")
    case "logout": osa("tell application \"loginwindow\" to «event aevtlogo»")
    case "restart": osa("tell application \"loginwindow\" to «event aevtrrst»")
    case "shutdown": osa("tell application \"loginwindow\" to «event aevtrsdn»")
    default: nil
    }
}

/// Keyboard layouts / input methods enabled in System Settings › Keyboard › Input Sources.
func enabledInputSources() -> [TISInputSource] {
    let filter = [kTISPropertyInputSourceIsSelectCapable: true,
                  kTISPropertyInputSourceCategory: kTISCategoryKeyboardInputSource!] as CFDictionary
    return TISCreateInputSourceList(filter, false)?.takeRetainedValue() as? [TISInputSource] ?? []
}

func inputSourceProperty(_ source: TISInputSource, _ key: CFString) -> String {
    guard let ptr = TISGetInputSourceProperty(source, key) else { return "" }
    return Unmanaged<CFString>.fromOpaque(ptr).takeUnretainedValue() as String
}

func inputSourceID(_ source: TISInputSource) -> String { inputSourceProperty(source, kTISPropertyInputSourceID) }

/// Enabled input sources as (id, name), for the editor picker. id is stored, e.g. "com.apple.keylayout.British".
func inputSourceChoices() -> [(id: String, name: String)] {
    enabledInputSources().map { (inputSourceID($0), inputSourceProperty($0, kTISPropertyLocalizedName)) }
}

func inputSourceName(_ id: String) -> String { inputSourceChoices().first { $0.id == id }?.name ?? id }

/// Switch to one specific input source by id.
func selectInputSource(_ id: String) {
    guard let source = enabledInputSources().first(where: { inputSourceID($0) == id }) else {
        return logger.error("Input source \(id, privacy: .public) not enabled")
    }
    let status = TISSelectInputSource(source)
    if status != noErr { logger.error("Input source switch failed: \(status)") }
}

/// Switch to the input source after the current one, wrapping around (like ⌃Space).
func selectNextInputSource() {
    let sources = enabledInputSources()
    guard sources.count > 1 else { return logger.error("Only one input source enabled") }
    let current = inputSourceID(TISCopyCurrentKeyboardInputSource().takeRetainedValue())
    let i = sources.firstIndex { inputSourceID($0) == current } ?? -1
    let next = sources[(i + 1) % sources.count]
    let status = TISSelectInputSource(next)
    if status != noErr { logger.error("Input source switch failed: \(status)") }
}
