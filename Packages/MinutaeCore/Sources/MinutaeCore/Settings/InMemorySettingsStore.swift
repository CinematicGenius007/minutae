import Foundation

/// In-memory `SettingsStore` for tests and previews. Copies share one storage object.
public struct InMemorySettingsStore: SettingsStore {
    private let storage: InMemorySettingsStorage

    public init() {
        storage = InMemorySettingsStorage(settings: .defaults)
    }

    public init(settings: AppSettings) throws {
        try settings.validate()
        storage = InMemorySettingsStorage(settings: settings)
    }

    public func load() async throws -> AppSettings {
        storage.load()
    }

    public func save(_ settings: AppSettings) async throws {
        try storage.save(settings)
    }

    public func changes() async -> AsyncStream<AppSettings> {
        storage.changes.stream()
    }
}

private final class InMemorySettingsStorage: @unchecked Sendable {
    private let lock = NSLock()
    private var settings: AppSettings
    let changes = SettingsChangeBroadcaster()

    init(settings: AppSettings) {
        self.settings = settings
    }

    func load() -> AppSettings {
        lock.lock()
        defer { lock.unlock() }
        return settings
    }

    func save(_ settings: AppSettings) throws {
        do {
            try settings.validate()
        } catch let error as SettingsValidationError {
            throw SettingsStoreError.invalid(error)
        } catch {
            throw SettingsStoreError.unreadable
        }
        lock.lock()
        self.settings = settings
        lock.unlock()
        changes.emit(settings)
    }
}
