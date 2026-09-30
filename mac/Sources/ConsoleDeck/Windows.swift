import SwiftUI
import UniformTypeIdentifiers

/// The 3-3-4 deck drawn as buttons; `label` and `highlight` decide what each key shows.
struct DeckGrid: View {
    var label: (Int) -> String
    var highlight: (Int) -> Color = { _ in .secondary.opacity(0.15) }
    var labelColor: (Int) -> Color = { _ in .secondary }
    var onTap: ((Int) -> Void)?

    var body: some View {
        VStack(spacing: 8) {
            ForEach(layout, id: \.self) { row in
                HStack(spacing: 8) {
                    ForEach(row, id: \.self) { n in
                        Button { onTap?(n) } label: {
                            VStack(spacing: 2) {
                                Text("\(n)").font(.headline)
                                Text(label(n)).font(.caption2).lineLimit(1).foregroundStyle(labelColor(n))
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
    @State private var profileID: String?  // nil = Default
    @State private var confirmDelete = false

    private static let kinds = [("none", "None"), ("url", "Open URL"), ("app", "Open App"), ("script", "Run Script")]

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            profileBar
            DeckGrid(
                label: { summary(effective($0)) },
                highlight: { n in n == selected ? .accentColor.opacity(0.6) : .secondary.opacity(0.15) },
                labelColor: { n in profile != nil && own(n) == nil ? .secondary.opacity(0.5) : .primary },
                onTap: { selected = $0 }
            )
            .frame(maxWidth: .infinity)
            Divider()
            Form {
                Picker("Button \(selected) does", selection: kind) {
                    if profile != nil { Text("Use Default").tag("inherit") }
                    ForEach(Self.kinds, id: \.0) { Text($0.1).tag($0.0) }
                }
                switch kind.wrappedValue {
                case "inherit":
                    LabeledContent("Default", value: describe(deck.config.buttons[String(selected)] ?? Action(type: "none")))
                case "url":
                    TextField("URL", text: value, prompt: Text("https://example.com"))
                case "app":
                    HStack {
                        TextField("App", text: value, prompt: Text("Safari"))
                        Button("Choose…") { if let app = pickApp() { value.wrappedValue = app.name } }
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
                Button("Test") { run(effective(selected)) }.disabled((effective(selected).value ?? "").isEmpty)
            }
        }
        .padding(20)
        .frame(width: 380)
        .onAppear {
            deck.reload()
            // start on the profile of the app you came from, if it has one
            profileID = deck.frontApp.flatMap { deck.config.profiles[$0.id] != nil ? $0.id : nil }
        }
        .confirmationDialog("Delete the \(profileName) profile?", isPresented: $confirmDelete) {
            Button("Delete", role: .destructive) {
                if let profile { deck.config.profiles[profile] = nil }
                deck.save()
                profileID = nil
            }
        } message: {
            Text("Its button actions are removed. Buttons go back to the default actions in that app.")
        }
    }

    private var profileBar: some View {
        HStack {
            Picker("Profile", selection: $profileID) {
                Label("Default", systemImage: "square.grid.3x3").tag(String?.none)
                ForEach(deck.config.profiles.sorted { $0.value.name < $1.value.name }, id: \.key) { id, p in
                    Label { Text(p.name) } icon: { AppIcon(bundleID: id) }.tag(Optional(id))
                }
            }
            Menu {
                ForEach(addableApps, id: \.id) { app in
                    Button { addProfile(app) } label: { Label { Text(app.name) } icon: { AppIcon(bundleID: app.id) } }
                }
                if !addableApps.isEmpty { Divider() }
                Button("Choose App…") { if let app = pickApp() { addProfile(app) } }
            } label: {
                Image(systemName: "plus")
            }
            .menuIndicator(.hidden)
            .fixedSize()
            .help("Add a profile for an app")
            Button { confirmDelete = true } label: { Image(systemName: "minus") }
                .disabled(profile == nil)
                .help("Delete this profile")
        }
    }

    /// Selected profile, or nil for Default (also when the profile was deleted elsewhere).
    private var profile: String? {
        profileID.flatMap { deck.config.profiles[$0] != nil ? $0 : nil }
    }

    private var profileName: String {
        profile.flatMap { deck.config.profiles[$0]?.name } ?? "Default"
    }

    private func own(_ n: Int) -> Action? { deck.config.ownAction(button: n, profile: profile) }

    /// What pressing button n does while this profile is active.
    private func effective(_ n: Int) -> Action {
        own(n) ?? deck.config.buttons[String(n)] ?? Action(type: "none")
    }

    private func setOwn(_ action: Action?) {
        deck.config.setAction(action, button: selected, profile: profile)
        deck.save()  // every edit is saved straight away, like deck.py
    }

    private var kind: Binding<String> {
        Binding {
            own(selected)?.type ?? (profile == nil ? "none" : "inherit")
        } set: { type in
            guard type != "inherit" else { return setOwn(nil) }
            var a = own(selected) ?? Action(type: type)
            a.type = type
            setOwn(a)
        }
    }

    private var value: Binding<String> {
        Binding { own(selected)?.value ?? "" } set: { v in
            var a = own(selected) ?? Action(type: "none")
            a.value = v
            setOwn(a)
        }
    }

    /// Regular running apps without a profile yet, for the + menu.
    private var addableApps: [FrontApp] {
        NSWorkspace.shared.runningApplications
            .filter { $0.activationPolicy == .regular && $0.bundleIdentifier != Bundle.main.bundleIdentifier }
            .compactMap(FrontApp.init)
            .filter { deck.config.profiles[$0.id] == nil }
            .sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }

    private func addProfile(_ app: FrontApp) {
        if deck.config.profiles[app.id] == nil {
            deck.config.profiles[app.id] = Profile(name: app.name)
            deck.save()
        }
        profileID = app.id
    }

    private func summary(_ action: Action) -> String {
        guard action.type != "none", let value = action.value, !value.isEmpty else { return "–" }
        return action.type == "url" ? (URL(string: value)?.host() ?? value) : value
    }
}

struct AppIcon: View {
    let bundleID: String

    var body: some View {
        if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) {
            let icon = NSWorkspace.shared.icon(forFile: url.path)
            let _ = icon.size = NSSize(width: 16, height: 16)  // menu-sized
            Image(nsImage: icon)
        } else {
            Image(systemName: "app.dashed")
        }
    }
}

/// Pick an app bundle from /Applications. Name is what `open -a` takes.
@MainActor
func pickApp() -> FrontApp? {
    let panel = NSOpenPanel()
    panel.allowedContentTypes = [.application]
    panel.directoryURL = URL(fileURLWithPath: "/Applications")
    guard panel.runModal() == .OK, let url = panel.url, let id = Bundle(url: url)?.bundleIdentifier else { return nil }
    return FrontApp(id: id, name: url.deletingPathExtension().lastPathComponent)
}
