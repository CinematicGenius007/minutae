import Foundation

/// Failure from a `SettingsStore`. Descriptions never include stored values.
public enum SettingsStoreError: Error, Equatable, Sendable {
    case invalid(SettingsValidationError)
    case unreadable
}

extension SettingsStoreError: CustomStringConvertible, LocalizedError {
    public var description: String {
        switch self {
        case .invalid(let error):
            return error.description
        case .unreadable:
            return "Settings could not be read."
        }
    }

    public var errorDescription: String? { description }
}

/// Persistence for `AppSettings`. A later database store can adopt the same protocol.
///
/// `changes()` emits values passed to `save` after the subscription starts.
public protocol SettingsStore: Sendable {
    func load() async throws -> AppSettings
    func save(_ settings: AppSettings) async throws
    func changes() async -> AsyncStream<AppSettings>
}

/// Fans validated settings out to subscribers. The lock guards the continuation list.
final class SettingsChangeBroadcaster: @unchecked Sendable {
    private let lock = NSLock()
    private var continuations: [UUID: AsyncStream<AppSettings>.Continuation] = [:]

    func stream() -> AsyncStream<AppSettings> {
        AsyncStream { continuation in
            let identifier = UUID()
            add(identifier, continuation)
            continuation.onTermination = { [weak self] _ in
                self?.remove(identifier)
            }
        }
    }

    func emit(_ settings: AppSettings) {
        let current = snapshot()
        for continuation in current {
            continuation.yield(settings)
        }
    }

    deinit {
        let current = snapshot(clearing: true)
        for continuation in current {
            continuation.finish()
        }
    }

    private func add(_ identifier: UUID, _ continuation: AsyncStream<AppSettings>.Continuation) {
        lock.lock()
        continuations[identifier] = continuation
        lock.unlock()
    }

    private func remove(_ identifier: UUID) {
        lock.lock()
        continuations.removeValue(forKey: identifier)
        lock.unlock()
    }

    private func snapshot(clearing: Bool = false) -> [AsyncStream<AppSettings>.Continuation] {
        lock.lock()
        let current = Array(continuations.values)
        if clearing {
            continuations.removeAll()
        }
        lock.unlock()
        return current
    }
}
