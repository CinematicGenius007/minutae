import os

/// In-memory permission service for tests and other packages.
///
/// Every permission starts as `notDetermined` unless `statuses` says otherwise.
/// `request(_:)` moves `notDetermined` to the scripted result, or to `granted`
/// when none was scripted. `granted`, `denied`, and `restricted` stay as they are.
/// Use `setStatus(_:for:)` to simulate the user changing System Settings later.
public final class FakePermissionService: PermissionService, Sendable {
    private struct State: Sendable {
        var statuses: [Permission: PermissionStatus]
        var requestResults: [Permission: PermissionStatus]
        var requestCounts: [Permission: Int] = [:]
    }

    private let state: OSAllocatedUnfairLock<State>
    private let emitted = OSAllocatedUnfairLock(initialState: [PermissionChange]())
    private let hub = PermissionChangeHub()

    public init(
        statuses: [Permission: PermissionStatus] = [:],
        requestResults: [Permission: PermissionStatus] = [:]
    ) {
        state = OSAllocatedUnfairLock(
            initialState: State(statuses: statuses, requestResults: requestResults)
        )
    }

    public func status(of permission: Permission) -> PermissionStatus {
        state.withLock { $0.statuses[permission] ?? .notDetermined }
    }

    public func request(_ permission: Permission) async -> PermissionStatus {
        let change: PermissionChange? = state.withLock { state in
            let before = state.statuses[permission] ?? .notDetermined
            guard before == .notDetermined else { return nil }
            state.requestCounts[permission, default: 0] += 1
            let after = state.requestResults[permission] ?? .granted
            state.statuses[permission] = after
            guard after != before else { return nil }
            return PermissionChange(permission: permission, status: after)
        }
        if let change {
            publish(change)
        }
        return status(of: permission)
    }

    public var changes: AsyncStream<PermissionChange> {
        hub.stream()
    }

    /// Replaces the stored status and emits when it actually changes.
    public func setStatus(_ status: PermissionStatus, for permission: Permission) {
        let change: PermissionChange? = state.withLock { state in
            let before = state.statuses[permission] ?? .notDetermined
            state.statuses[permission] = status
            guard status != before else { return nil }
            return PermissionChange(permission: permission, status: status)
        }
        if let change {
            publish(change)
        }
    }

    public func setRequestResult(_ status: PermissionStatus, for permission: Permission) {
        state.withLock { $0.requestResults[permission] = status }
    }

    func requestCount(for permission: Permission) -> Int {
        state.withLock { $0.requestCounts[permission] ?? 0 }
    }

    func emittedChanges() -> [PermissionChange] {
        emitted.withLock { $0 }
    }

    private func publish(_ change: PermissionChange) {
        emitted.withLock { $0.append(change) }
        hub.emit(change)
    }
}
