import Foundation

/// In-memory `KeychainStore` for tests. Secrets stay in the process and are not logged.
public struct InMemoryKeychainStore: KeychainStore {
    private let storage = InMemoryKeychainStorage()

    public init() {}

    public func setSecret(_ secret: String, for account: APIKeyAccount) async throws {
        try storage.setSecret(secret, for: account)
    }

    public func secret(for account: APIKeyAccount) async throws -> String? {
        storage.secret(for: account)
    }

    public func deleteSecret(for account: APIKeyAccount) async throws {
        storage.deleteSecret(for: account)
    }

    public func removeAll() async throws {
        storage.removeAll()
    }
}

private final class InMemoryKeychainStorage: @unchecked Sendable {
    private let lock = NSLock()
    private var secrets: [APIKeyAccount: String] = [:]

    func setSecret(_ secret: String, for account: APIKeyAccount) throws {
        let data = try SecretRules.utf8Data(from: secret)
        guard let stored = String(bytes: data, encoding: .utf8) else {
            throw KeychainStoreError.rejectedSecret
        }
        lock.lock()
        secrets[account] = stored
        lock.unlock()
    }

    func secret(for account: APIKeyAccount) -> String? {
        lock.lock()
        defer { lock.unlock() }
        return secrets[account]
    }

    func deleteSecret(for account: APIKeyAccount) {
        lock.lock()
        secrets.removeValue(forKey: account)
        lock.unlock()
    }

    func removeAll() {
        lock.lock()
        secrets.removeAll()
        lock.unlock()
    }
}
