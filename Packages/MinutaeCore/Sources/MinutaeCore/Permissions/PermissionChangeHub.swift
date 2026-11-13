import Foundation
import os

/// Fans one status change out to every current subscriber.
final class PermissionChangeHub: Sendable {
    private let subscribers = OSAllocatedUnfairLock(
        initialState: [UUID: AsyncStream<PermissionChange>.Continuation]()
    )

    func stream() -> AsyncStream<PermissionChange> {
        AsyncStream { continuation in
            let id = UUID()
            subscribers.withLock { $0[id] = continuation }
            continuation.onTermination = { [subscribers] _ in
                subscribers.withLock { $0[id] = nil }
            }
        }
    }

    func emit(_ change: PermissionChange) {
        let continuations = subscribers.withLock { Array($0.values) }
        for continuation in continuations {
            continuation.yield(change)
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
