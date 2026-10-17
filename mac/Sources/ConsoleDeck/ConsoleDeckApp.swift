import ServiceManagement
import SwiftUI

@main
struct ConsoleDeckApp: App {
    @State private var deck = Deck()

    var body: some Scene {
        MenuBarExtra {
            MenuContent(deck: deck)
        } label: {
            MenuBarIcon(deck: deck)
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

/// Drawn at launch (unlike the menu), so it hands the deck SwiftUI's window opener for "Open ConsoleDeck Editor".
struct MenuBarIcon: View {
    let deck: Deck
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Image(systemName: "square.grid.3x3.fill")
            .onAppear { deck.openWindow = openWindow }
    }
}

struct MenuContent: View {
    let deck: Deck

    var body: some View {
        Text(deck.status)
        Text("Profile: \(deck.profileName)" + (deck.frontApp.map { "  (\($0.name) in front)" } ?? ""))
        if !deck.lastPress.isEmpty { Text(deck.lastPress) }
        if let combo = deck.repeating {
            Button("Stop Repeating \(keyComboSymbols(combo))") { deck.toggleRepeat(combo) }
        }
        Divider()
        Button("Edit Actions…") { deck.show("actions") }
        Button("Calibrate Buttons…") { deck.show("calibrate") }
        Divider()
        Toggle("Start at Login", isOn: Binding { deck.startsAtLogin } set: { deck.setStartsAtLogin($0) })
        Button("Show Config in Finder") { NSWorkspace.shared.activateFileViewerSelecting([Config.url]) }
        Button("Quit ConsoleDeck") { NSApp.terminate(nil) }.keyboardShortcut("q")
    }
}
