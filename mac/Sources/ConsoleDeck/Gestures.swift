import Foundation

enum Gesture: String, CaseIterable {
    case press, long, double

    var label: String {
        switch self {
        case .press: "Press"
        case .long: "Long Press"
        case .double: "Double Press"
        }
    }

    /// Key in deck.json's buttons: "3", "3.long", "3.double". deck.py only knows plain "3".
    func key(_ n: Int) -> String { self == .press ? "\(n)" : "\(n).\(rawValue)" }
}

/// Turns raw down/up events into press, long press and double press.
/// A button only waits (to tell gestures apart) when it has a long or double action;
/// otherwise it fires on the way down, with no added delay.
@MainActor
final class GestureDetector {
    typealias Schedule = @MainActor (TimeInterval, @escaping @MainActor () -> Void) -> () -> Void

    var longPress: TimeInterval = 0.5
    var doubleGap: TimeInterval = 0.3

    private let schedule: Schedule
    private let fire: @MainActor (Int, Gesture) -> Void
    private var uses: [Int: Set<Gesture>] = [:]   // gestures in play for a button that's down
    private var longTimer: [Int: () -> Void] = [:]
    private var pendingPress: [Int: () -> Void] = [:]  // single press waiting to see if a second one comes
    private var handled: Set<Int> = []  // this down already fired; ignore its release

    init(schedule: @escaping Schedule = GestureDetector.after, fire: @escaping @MainActor (Int, Gesture) -> Void) {
        self.schedule = schedule
        self.fire = fire
    }

    /// `uses`: which of .long / .double the button has actions for right now.
    func down(_ n: Int, uses: Set<Gesture>) {
        if let cancel = pendingPress.removeValue(forKey: n) {
            cancel()
            handled.insert(n)
            return fire(n, .double)
        }
        guard !uses.isEmpty else {
            handled.insert(n)
            return fire(n, .press)
        }
        self.uses[n] = uses
        if uses.contains(.long) {
            longTimer[n] = schedule(longPress) { [weak self] in
                guard let self else { return }
                longTimer[n] = nil
                handled.insert(n)
                fire(n, .long)
            }
        }
    }

    func up(_ n: Int) {
        longTimer.removeValue(forKey: n)?()
        let uses = uses.removeValue(forKey: n)
        if handled.remove(n) != nil { return }
        guard let uses else { return }  // release of a press we never saw (port opened mid-hold)
        if uses.contains(.double) {
            pendingPress[n] = schedule(doubleGap) { [weak self] in
                self?.pendingPress[n] = nil
                self?.fire(n, .press)
            }
        } else {
            fire(n, .press)
        }
    }

    static func after(_ delay: TimeInterval, _ fn: @escaping @MainActor () -> Void) -> () -> Void {
        let task = Task { @MainActor in
            try? await Task.sleep(for: .seconds(delay))
            if !Task.isCancelled { fn() }
        }
        return { task.cancel() }
    }
}
