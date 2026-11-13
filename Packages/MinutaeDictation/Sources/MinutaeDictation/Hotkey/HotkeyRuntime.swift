import Foundation
import os

/// Fans one event out to every current subscriber. Subscribers do not replay.
final class HotkeyEventHub<Event: Sendable>: Sendable {
    private let subscribers = OSAllocatedUnfairLock(
        initialState: [UUID: AsyncStream<Event>.Continuation]()
    )

    func stream() -> AsyncStream<Event> {
        AsyncStream { continuation in
            let id = UUID()
            subscribers.withLock { $0[id] = continuation }
            continuation.onTermination = { [subscribers] _ in
                subscribers.withLock { $0[id] = nil }
            }
        }
    }

    func emit(_ event: Event) {
        let continuations = subscribers.withLock { Array($0.values) }
        for continuation in continuations {
            continuation.yield(event)
        }
    }

    deinit {
        let continuations = subscribers.withLock { subscribers in
            let values = Array(subscribers.values)
            subscribers.removeAll()
            return values
        }
        for continuation in continuations {
            continuation.finish()
        }
    }
}

/// Registration, the session machine, and the two streams shared by the Carbon
/// service and the fake.
final class HotkeyRuntime: Sendable {
    private struct State: Sendable {
        var registration: HotkeyRegistration?
        var machine = HotkeySessionMachine(mode: .hold)
    }

    private struct Trace: Sendable {
        var events: [HotkeyEvent] = []
        var session: [HotkeySessionEdge] = []
    }

    private let recordsTrace: Bool
    private let state = OSAllocatedUnfairLock(initialState: State())
    private let trace = OSAllocatedUnfairLock(initialState: Trace())
    private let events = HotkeyEventHub<HotkeyEvent>()
    private let session = HotkeyEventHub<HotkeySessionEdge>()

    init(recordsTrace: Bool = false) {
        self.recordsTrace = recordsTrace
    }

    var registration: HotkeyRegistration? {
        state.withLock { $0.registration }
    }

    var isActive: Bool {
        state.withLock { $0.machine.isActive }
    }

    func eventsStream() -> AsyncStream<HotkeyEvent> {
        events.stream()
    }

    func sessionStream() -> AsyncStream<HotkeySessionEdge> {
        session.stream()
    }

    /// Stores a successful registration. A change away from an active session
    /// emits `.ended` and the machine starts idle in the new mode.
    func apply(_ registration: HotkeyRegistration) {
        let edge: HotkeySessionEdge? = state.withLock { state in
            let previous = state.registration
            guard previous != registration else { return nil }
            let wasActive = state.machine.isActive
            state.registration = registration
            state.machine = HotkeySessionMachine(mode: registration.mode)
            return wasActive ? .ended : nil
        }
        if let edge {
            record(session: edge)
            session.emit(edge)
        }
    }

    /// Drops the registration. An active session ends. No synthetic key-up is emitted.
    func clear() {
        let edge: HotkeySessionEdge? = state.withLock { state in
            let wasActive = state.machine.isActive
            state.registration = nil
            state.machine = HotkeySessionMachine(mode: state.machine.mode)
            return wasActive ? .ended : nil
        }
        if let edge {
            record(session: edge)
            session.emit(edge)
        }
    }

    /// Feeds one physical phase. Returns false when nothing is armed.
    @discardableResult
    func receive(_ phase: HotkeyPhase) -> Bool {
        let delivered: (HotkeyEvent, HotkeySessionEdge?)? = state.withLock { state in
            guard let registration = state.registration else { return nil }
            let edge = state.machine.handle(phase)
            return (HotkeyEvent(phase: phase, chord: registration.chord), edge)
        }
        guard let delivered else { return false }
        record(event: delivered.0, session: delivered.1)
        events.emit(delivered.0)
        if let edge = delivered.1 {
            session.emit(edge)
        }
        return true
    }

    func emittedEvents() -> [HotkeyEvent] {
        trace.withLock { $0.events }
    }

    func emittedSession() -> [HotkeySessionEdge] {
        trace.withLock { $0.session }
    }

    private func record(event: HotkeyEvent, session edge: HotkeySessionEdge?) {
        guard recordsTrace else { return }
        trace.withLock { trace in
            trace.events.append(event)
            if let edge {
                trace.session.append(edge)
            }
        }
    }

    private func record(session edge: HotkeySessionEdge) {
        guard recordsTrace else { return }
        trace.withLock { $0.session.append(edge) }
    }
}
