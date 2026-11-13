import Foundation
import LocalAuthentication
import Security

/// Security-framework generic passwords. Queries always set `kSecAttrService`, so other
/// apps' items are not read. Interaction is refused; a required prompt becomes `.unavailable`.
public struct SecurityKeychainStore: KeychainStore {
    public static let productionService = "com.nebula.minutae"

    public let service: String

    public init(service: String = productionService) throws {
        try ServiceName.validate(service)
        self.service = service
    }

    public func setSecret(_ secret: String, for account: APIKeyAccount) async throws {
        let service = self.service
        try await Task.detached(priority: .utility) {
            try KeychainDatabase.setSecret(secret, account: account, service: service)
        }.value
    }

    public func secret(for account: APIKeyAccount) async throws -> String? {
        let service = self.service
        return try await Task.detached(priority: .utility) {
            try KeychainDatabase.secret(account: account, service: service)
        }.value
    }

    public func deleteSecret(for account: APIKeyAccount) async throws {
        let service = self.service
        try await Task.detached(priority: .utility) {
            try KeychainDatabase.deleteSecret(account: account, service: service)
        }.value
    }

    public func removeAll() async throws {
        let service = self.service
        try await Task.detached(priority: .utility) {
            try KeychainDatabase.removeAll(service: service)
        }.value
    }
}

enum KeychainDatabase {
    static func setSecret(_ secret: String, account: APIKeyAccount, service: String) throws {
        let data = try SecretRules.utf8Data(from: secret)
        var query = baseQuery(account: account.rawValue, service: service)
        query[kSecValueData as String] = data
        query[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly

        let status = SecItemAdd(query as CFDictionary, nil)
        if status == errSecSuccess {
            return
        }
        if status == errSecDuplicateItem {
            let attributes = [kSecValueData as String: data]
            let updateQuery = baseQuery(account: account.rawValue, service: service)
            try check(SecItemUpdate(updateQuery as CFDictionary, attributes as CFDictionary))
            return
        }
        try check(status)
    }

    static func secret(account: APIKeyAccount, service: String) throws -> String? {
        var query = baseQuery(account: account.rawValue, service: service)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne

        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        if status == errSecItemNotFound {
            return nil
        }
        try check(status)
        guard let data = item as? Data, let secret = String(data: data, encoding: .utf8) else {
            throw KeychainStoreError.unreadable
        }
        return secret
    }

    static func deleteSecret(account: APIKeyAccount, service: String) throws {
        let status = SecItemDelete(
            baseQuery(account: account.rawValue, service: service) as CFDictionary)
        if status == errSecSuccess || status == errSecItemNotFound {
            return
        }
        try check(status)
    }

    static func removeAll(service: String) throws {
        let status = SecItemDelete(baseQuery(account: nil, service: service) as CFDictionary)
        if status == errSecSuccess || status == errSecItemNotFound {
            return
        }
        try check(status)
    }

    private static func baseQuery(account: String?, service: String) -> [String: Any] {
        let context = LAContext()
        context.interactionNotAllowed = true
        var query: [String: Any] = [:]
        query[kSecClass as String] = kSecClassGenericPassword
        query[kSecAttrService as String] = service
        query[kSecAttrSynchronizable as String] = false
        query[kSecUseAuthenticationContext as String] = context
        if let account {
            query[kSecAttrAccount as String] = account
        }
        return query
    }

    private static func check(_ status: OSStatus) throws {
        if status == errSecSuccess {
            return
        }
        throw map(status)
    }

    private static func map(_ status: OSStatus) -> KeychainStoreError {
        switch status {
        case errSecNotAvailable, errSecInteractionNotAllowed, errSecMissingEntitlement,
            errSecUserCanceled, errSecAuthFailed:
            return .unavailable
        default:
            return .failed(status)
        }
    }
}
