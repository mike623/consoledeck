import AppKit
import SwiftUI

/// Floating, click-through panel showing what every button does in the app in front.
/// Non-activating, so the front app (and its profile) stays put.
@MainActor
func makeCheatSheet(_ config: Config, app: FrontApp?) -> NSPanel {
    let hosting = NSHostingView(rootView: CheatSheetView(config: config, app: app))
    let panel = NSPanel(contentRect: NSRect(origin: .zero, size: hosting.fittingSize),
                        styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
    panel.contentView = hosting
    panel.isOpaque = false
    panel.backgroundColor = .clear
    panel.hasShadow = false  // window shadow is square and outlines the rounded panel
    panel.level = .statusBar  // above full-screen games and streams
    panel.ignoresMouseEvents = true
    panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
    if let screen = NSScreen.main?.visibleFrame {
        panel.setFrameOrigin(NSPoint(x: screen.midX - panel.frame.width / 2, y: screen.midY - panel.frame.height / 2))
    }
    panel.orderFrontRegardless()
    return panel
}

struct CheatSheetView: View {
    let config: Config
    let app: FrontApp?

    var body: some View {
        VStack(spacing: 12) {
            Text(app.flatMap { config.profiles[$0.id]?.name } ?? "Default").font(.headline)
            VStack(spacing: 8) {
                ForEach(layout, id: \.self) { row in
                    HStack(spacing: 8) {
                        ForEach(row, id: \.self, content: key)
                    }
                }
            }
        }
        .padding(20)
        .background(.ultraThickMaterial, in: .rect(cornerRadius: 16))
    }

    private func key(_ n: Int) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("\(n)").font(.caption.bold()).foregroundStyle(.secondary)
            ForEach(Gesture.allCases, id: \.self) { g in
                let label = shortLabel(config.action(button: n, gesture: g, app: app?.id).action)
                if g == .press || label != "–" {
                    Text([.long: "Long: ", .double: "Double: "][g, default: ""] + label).lineLimit(1)
                }
            }
        }
        .font(.callout)
        .frame(width: 130, height: 70, alignment: .topLeading)
        .padding(8)
        .background(.secondary.opacity(0.15), in: .rect(cornerRadius: 10))
    }
}
