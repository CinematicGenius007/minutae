import os

/// In-memory hotkey service for tests and other packages.
///
/// `systemStatus` is the Carbon `OSStatus` a chord change would receive.
/// Zero accepts the chord. A non-zero status fails registration and leaves the
/// previous chord armed. Invalid chords fail before that status is consulted.
public final class FakeHotkeyService: HotkeyService, Sendable {
    private struct Counters: Sendable {
        var systemStatus: Int32
        var registerCalls = 0
        var systemRegisterCalls = 0
    }

    private let runtime = HotkeyRuntime(recordsTrace: true)
    private let counters: OSAllocatedUnfairLock<Counters>

    public init(systemStatus: Int32 = HotkeyCarbonStatus.success) {
        counters = OSAllocatedUnfairLock(initialState: Counters(systemStatus: systemStatus))
    }

    public var events: AsyncStream<HotkeyEvent> {
        runtime.eventsStream()
    }

    public var session: AsyncStream<HotkeySessionEdge> {
        runtime.sessionStream()
    }

    public var registration: HotkeyRegistration? {
        runtime.registration
    }

    public var isActive: Bool {
        runtime.isActive
    }

    public func setSystemStatus(_ status: Int32) {
        counters.withLock { $0.systemStatus = status }
    }

    public func register(_ registration: HotkeyRegistration) throws {
        if let problem = HotkeyChordValidation.problem(with: registration.chord) {
            counters.withLock { $0.registerCalls += 1 }
            throw HotkeyRegistrationFailure.reporting(registration.chord, reason: .invalid(problem))
        }

        let outcome: Result<Void, HotkeyRegistrationFailure>? = counters.withLock { counters in
            counters.registerCalls += 1
            let chordChanges = runtime.registration?.chord != registration.chord
            guard chordChanges else { return nil }
            counters.systemRegisterCalls += 1
            let status = counters.systemStatus
            guard status != HotkeyCarbonStatus.success else { return nil }
            return .failure(.reporting(registration.chord, reason: .system(status)))
        }
        if case .failure(let failure)? = outcome {
            throw failure
        }
        runtime.apply(registration)
    }

    public func unregister() {
        runtime.clear()
    }

    /// Injects a press or release as if Carbon had delivered it.
    /// Returns false when no chord is armed.
    @discardableResult
    public func simulate(_ phase: HotkeyPhase) -> Bool {
        runtime.receive(phase)
    }

    func emittedEvents() -> [HotkeyEvent] {
        runtime.emittedEvents()
    }

    func emittedSession() -> [HotkeySessionEdge] {
        runtime.emittedSession()
    }

    func registerCallCount() -> Int {
        counters.withLock { $0.registerCalls }
    }

    func systemRegisterCallCount() -> Int {
        counters.withLock { $0.systemRegisterCalls }
    }
}
