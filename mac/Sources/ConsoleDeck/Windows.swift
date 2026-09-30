import SwiftUI
import UniformTypeIdentifiers

/// The 3-3-4 deck drawn as buttons; `label` and `highlight` decide what each key shows.
struct DeckGrid: View {
    var label: (Int) -> String
    var highlight: (Int) -> Color = { _ in .secondary.opacity(0.15) }
    var onTap: ((Int) -> Void)?

    var body: some View {
        VStack(spacing: 8) {
            ForEach(layout, id: \.self) { row in
                HStack(spacing: 8) {
                    ForEach(row, id: \.self) { n in
                        Button { onTap?(n) } label: {
                            VStack(spacing: 2) {
                                Text("\(n)").font(.headline)
                                Text(label(n)).font(.caption2).lineLimit(1).foregroundStyle(.secondary)
                            }
                            .frame(width: 72, height: 52)
                            .background(highlight(n), in: .rect(cornerRadius: 8))
                        }
                        .buttonStyle(.plain)
                        .disabled(onTap == nil)
                    }
                }
            }
        }
    }
}

struct CalibrateView: View {
    @Bindable var deck: Deck

    var body: some View {
        VStack(spacing: 16) {
            Text(deck.calibration == nil
                 ? "Press Start, then press each deck button when it lights up. Don't touch the knob."
                 : "Press the highlighted button on the deck.")
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
            DeckGrid(label: label, highlight: highlight)
            Text(deck.calibrationNote.isEmpty ? deck.status : deck.calibrationNote)
                .font(.callout).foregroundStyle(.secondary)
            HStack {
                if deck.calibration == nil {
                    Button("Start") { deck.startCalibration() }.keyboardShortcut(.defaultAction)
                } else {
                    Button("Cancel") { deck.cancelCalibration() }.keyboardShortcut(.cancelAction)
                }
            }
        }
        .padding(20)
        .frame(width: 300)
        .onDisappear { deck.cancelCalibration() }  // closing the window must not leave presses swallowed
    }

    private func label(_ n: Int) -> String {
        if let cal = deck.calibration {
            return cal.first { $0.value == n }?.key ?? "–"
        }
        return deck.config.pin(forButton: n) ?? "not set"
    }

    private func highlight(_ n: Int) -> Color {
        guard let cal = deck.calibration else { return .secondary.opacity(0.15) }
        if n == deck.nextToCalibrate { return .accentColor.opacity(0.6) }
        return cal.values.contains(n) ? .green.opacity(0.3) : .secondary.opacity(0.15)
    }
}

struct ActionsView: View {
    @Bindable var deck: Deck
    @State private var selected = 1

    private static let kinds = [("none", "None"), ("url", "Open URL"), ("app", "Open App"), ("script", "Run Script")]

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            DeckGrid(
                label: { n in summary(deck.config.buttons[String(n)]) },
                highlight: { n in n == selected ? .accentColor.opacity(0.6) : .secondary.opacity(0.15) },
                onTap: { selected = $0 }
            )
            .frame(maxWidth: .infinity)
            Divider()
            Form {
                Picker("Button \(selected) does", selection: kind) {
                    ForEach(Self.kinds, id: \.0) { Text($0.1).tag($0.0) }
                }
                switch action.type {
                case "url":
                    TextField("URL", text: value, prompt: Text("https://example.com"))
                case "app":
                    HStack {
                        TextField("App", text: value, prompt: Text("Safari"))
                        Button("Choose…", action: chooseApp)
                    }
                case "script":
                    TextField("Command", text: value, prompt: Text("~/bin/thing.sh"), axis: .vertical)
                        .lineLimit(3...8)
                        .font(.body.monospaced())
                default:
                    EmptyView()
                }
            }
            HStack {
                if deck.config.pin(forButton: selected) == nil {
                    Label("Not calibrated yet", systemImage: "exclamationmark.triangle").foregroundStyle(.orange)
                }
                Spacer()
                Button("Test") { run(action) }.disabled((action.value ?? "").isEmpty)
            }
        }
        .padding(20)
        .frame(width: 360)
        .onAppear { deck.reload() }
    }

    private var action: Action { deck.config.buttons[String(selected)] ?? Action(type: "none") }

    private var value: Binding<String> {
        Binding { action.value ?? "" } set: { v in edit { $0.value = v } }
    }

    private var kind: Binding<String> {
        Binding { action.type } set: { t in edit { $0.type = t } }
    }

    /// Every edit is saved straight away, like deck.py.
    private func edit(_ change: (inout Action) -> Void) {
        var a = action
        change(&a)
        deck.config.buttons[String(selected)] = a
        deck.save()
    }

    private func summary(_ action: Action?) -> String {
        guard let action, action.type != "none", let value = action.value, !value.isEmpty else { return "–" }
        return action.type == "url" ? (URL(string: value)?.host() ?? value) : value
    }

    private func chooseApp() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.application]
        panel.directoryURL = URL(fileURLWithPath: "/Applications")
        if panel.runModal() == .OK, let url = panel.url {
            value.wrappedValue = url.deletingPathExtension().lastPathComponent
        }
    }
}
