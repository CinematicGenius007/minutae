import Foundation

/// `SettingsStore` backed by an injectable `UserDefaults` suite.
public struct UserDefaultsSettingsStore: SettingsStore {
    /// Suite name for the app. Tests pass their own suite instead of this one.
    public static let suiteName = "com.nebula.minutae"
    public static let storageKey = "appSettings"

    private let storage: UserDefaultsSettingsStorage

    public init(userDefaults: UserDefaults, storageKey: String = Self.storageKey) {
        storage = UserDefaultsSettingsStorage(userDefaults: userDefaults, storageKey: storageKey)
    }

    public func load() async throws -> AppSettings {
        try storage.load()
    }

    public func save(_ settings: AppSettings) async throws {
        try storage.save(settings)
    }

    public func changes() async -> AsyncStream<AppSettings> {
        storage.changes.stream()
    }
}

/// `UserDefaults` is thread-safe. Continuations live in `SettingsChangeBroadcaster`.
private final class UserDefaultsSettingsStorage: @unchecked Sendable {
    private let userDefaults: UserDefaults
    private let storageKey: String
    let changes = SettingsChangeBroadcaster()

    init(userDefaults: UserDefaults, storageKey: String) {
        self.userDefaults = userDefaults
        self.storageKey = storageKey
    }

    func load() throws -> AppSettings {
        guard let data = userDefaults.data(forKey: storageKey) else {
            return .defaults
        }
        let settings: AppSettings
        do {
            settings = try JSONDecoder().decode(AppSettings.self, from: data)
        } catch {
            throw SettingsStoreError.unreadable
        }
        do {
            try settings.validate()
        } catch let error as SettingsValidationError {
            throw SettingsStoreError.invalid(error)
        } catch {
            throw SettingsStoreError.unreadable
        }
        return settings
    }

    func save(_ settings: AppSettings) throws {
        let data: Data
        do {
            try settings.validate()
            data = try JSONEncoder().encode(settings)
        } catch let error as SettingsValidationError {
            throw SettingsStoreError.invalid(error)
        } catch {
            throw SettingsStoreError.unreadable
        }
        userDefaults.set(data, forKey: storageKey)
        changes.emit(settings)
    }
}
