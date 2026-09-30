import ServiceManagement
import SwiftUI

@main
struct ConsoleDeckApp: App {
    @State private var deck = Deck()

    var body: some Scene {
        MenuBarExtra("ConsoleDeck", systemImage: "square.grid.3x3.fill") {
            MenuContent(deck: deck)
        }
        Window("Calibrate ConsoleDeck", id: "calibrate") {
            CalibrateView(deck: deck)
        }
        .windowResizability(.contentSize)
        .defaultLaunchBehavior(.suppressed)
        Window("ConsoleDeck Actions", id: "actions") {
            ActionsView(deck: deck)
        }
        .windowResizability(.contentSize)
        .defaultLaunchBehavior(.suppressed)
    }
}

struct MenuContent: View {
    let deck: Deck
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Text(deck.status)
        Text("Profile: \(deck.profileName)" + (deck.frontApp.map { "  (\($0.name) in front)" } ?? ""))
        if !deck.lastPress.isEmpty { Text(deck.lastPress) }
        Divider()
        Button("Edit Actions…") { show("actions") }
        Button("Calibrate Buttons…") { show("calibrate") }
        Divider()
        Toggle("Start at Login", isOn: Binding { deck.startsAtLogin } set: { deck.setStartsAtLogin($0) })
        Button("Show Config in Finder") { NSWorkspace.shared.activateFileViewerSelecting([Config.url]) }
        Button("Quit ConsoleDeck") { NSApp.terminate(nil) }.keyboardShortcut("q")
    }

    private func show(_ id: String) {
        openWindow(id: id)
        NSApp.activate()  // menu bar apps don't come to the front on their own
    }
}
