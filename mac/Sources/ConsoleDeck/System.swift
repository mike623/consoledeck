import Foundation

/// System actions: stored value, menu label. Order is the order shown in the editor.
let systemActions: [(id: String, label: String)] = [
    ("lock", "Lock Screen"),
    ("sleep", "Sleep"),
    ("displaysleep", "Turn Display Off"),
    ("screensaver", "Start Screen Saver"),
    ("mute", "Mute / Unmute"),
    ("volumeup", "Volume Up"),
    ("volumedown", "Volume Down"),
    ("logout", "Log Out…"),
    ("restart", "Restart…"),
    ("shutdown", "Shut Down…"),
]

/// Command for a system action, or nil for "lock" (a key press) and unknown ids.
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
