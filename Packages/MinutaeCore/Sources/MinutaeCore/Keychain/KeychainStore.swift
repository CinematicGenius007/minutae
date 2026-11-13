import Foundation

/// Failure from a `KeychainStore`. Cases carry no secret material, and descriptions
/// never interpolate caller-supplied strings.
public enum KeychainStoreError: Error, Equatable, Sendable {
    case unavailable
    case rejectedSecret
    case rejectedService
    case unreadable
    case failed(OSStatus)
}

extension KeychainStoreError: CustomStringConvertible, LocalizedError {
    public var description: String {
        switch self {
        case .unavailable:
            return "Keychain is unavailable."
        case .rejectedSecret:
            return "Keychain secret was rejected."
        case .rejectedService:
            return "Keychain service name was rejected."
        case .unreadable:
            return "Keychain item could not be read."
        case .failed(let status):
            return "Keychain operation failed (\(status))."
        }
    }

    public var errorDescription: String? { description }
}

/// Generic-password storage for API transport keys.
///
/// Implementations must not log secrets or put them in errors. A missing item is `nil`.
/// Deleting a missing item succeeds.
public protocol KeychainStore: Sendable {
    func setSecret(_ secret: String, for account: APIKeyAccount) async throws
    func secret(for account: APIKeyAccount) async throws -> String?
    func deleteSecret(for account: APIKeyAccount) async throws
    func removeAll() async throws
}

enum SecretRules {
    static func utf8Data(from secret: String) throws -> Data {
        if secret.isEmpty || secret.unicodeScalars.contains(where: isDisallowed) {
            throw KeychainStoreError.rejectedSecret
        }
        guard let data = secret.data(using: .utf8) else {
            throw KeychainStoreError.rejectedSecret
        }
        return data
    }

    private static func isDisallowed(_ scalar: Unicode.Scalar) -> Bool {
        if scalar.value < 0x21 || scalar.value == 0x7F {
            return true
        }
        switch scalar.properties.generalCategory {
        case .control, .lineSeparator, .paragraphSeparator:
            return true
        default:
            return false
        }
    }
}

enum ServiceName {
    static let maxLength = 200

    static func validate(_ service: String) throws {
        if service.isEmpty || service.count > maxLength || service.contains("..") {
            throw KeychainStoreError.rejectedService
        }
        let allowed = CharacterSet(
            charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789.-"
        )
        if service.unicodeScalars.contains(where: { !allowed.contains($0) }) {
            throw KeychainStoreError.rejectedService
        }
    }
}
