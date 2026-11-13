import Foundation
import MinutaeDictation
import Observation

/// HUD state driven by `DictationController` events. It knows nothing about windows, so it is
/// tested without a panel. `onChange` fires once per visible change, including auto-dismiss.
@MainActor
@Observable
public final class DictationHUDModel {
    public private(set) var display: DictationHUDDisplay?
    /// When the current listening run started, for the elapsed timer.
    public private(set) var listeningSince: Date?

    @ObservationIgnored public var onChange: ((DictationHUDDisplay?) -> Void)?
    @ObservationIgnored private let dismissAfter: Duration
    @ObservationIgnored private let now: @Sendable () -> Date
    @ObservationIgnored private let sleep: @Sendable (Duration) async -> Void
    @ObservationIgnored private var dismissal: Task<Void, Never>?
    @ObservationIgnored private var generation = 0

    public init(
        dismissAfter: Duration = .milliseconds(1500),
        now: @escaping @Sendable () -> Date = { Date() },
        sleep: @escaping @Sendable (Duration) async -> Void = { try? await Task.sleep(for: $0) }
    ) {
        self.dismissAfter = dismissAfter
        self.now = now
        self.sleep = sleep
    }

    public func apply(_ event: DictationControllerEvent) {
        guard case .state(let state) = event else { return }
        show(DictationHUDDisplay.from(state))
    }

    /// Reads events until the stream ends or the calling task is cancelled.
    public func run(_ events: AsyncStream<DictationControllerEvent>) async {
        for await event in events {
            apply(event)
        }
    }

    func show(_ next: DictationHUDDisplay?) {
        guard next != display else { return }
        if next == .listening, display != .listening {
            listeningSince = now()
        } else if next != .listening {
            listeningSince = nil
        }
        display = next
        generation += 1
        dismissal?.cancel()
        dismissal = nil
        if let next, next.isTerminal {
            scheduleDismissal(generation)
        }
        onChange?(next)
    }

    private func scheduleDismissal(_ token: Int) {
        let delay = dismissAfter
        let sleep = sleep
        dismissal = Task { [weak self] in
            await sleep(delay)
            guard !Task.isCancelled, let self, self.generation == token else { return }
            self.display = nil
            self.generation += 1
            self.dismissal = nil
            self.onChange?(nil)
        }
    }
}
