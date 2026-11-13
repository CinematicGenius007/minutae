import Foundation
import Testing

@testable import MinutaeCore

@Test func inMemoryKeychainIsolatesAccounts() async throws {
    let store = InMemoryKeychainStore()
    let secret = "minutae-test-secret"
    try await store.setSecret(secret, for: .anthropic)
    #expect(try await store.secret(for: .anthropic) == secret)
    #expect(try await store.secret(for: .openai) == nil)
    #expect(try await store.secret(for: .gemini) == nil)

    try await store.setSecret("updated-secret", for: .anthropic)
    try await store.setSecret("other-secret", for: .openai)
    #expect(try await store.secret(for: .anthropic) == "updated-secret")
    #expect(try await store.secret(for: .openai) == "other-secret")

    try await store.deleteSecret(for: .anthropic)
    try await store.deleteSecret(for: .anthropic)
    #expect(try await store.secret(for: .anthropic) == nil)
    #expect(try await store.secret(for: .openai) == "other-secret")

    try await store.removeAll()
    #expect(try await store.secret(for: .openai) == nil)
}

@Test func keychainRejectsEmptyAndUnsafeSecretsWithoutEchoingThem() async throws {
    let store = InMemoryKeychainStore()
    let secret = "sentinel-value-should-not-leak"
    await #expect(throws: KeychainStoreError.rejectedSecret) {
        try await store.setSecret("", for: .gemini)
    }
    await #expect(throws: KeychainStoreError.rejectedSecret) {
        try await store.setSecret("   ", for: .gemini)
    }
    do {
        try await store.setSecret("\n\(secret)", for: .gemini)
        Issue.record("Expected the secret to be rejected.")
    } catch {
        #expect(error as? KeychainStoreError == .rejectedSecret)
        #expect(rendered(error).contains(secret) == false)
    }
    #expect(try await store.secret(for: .gemini) == nil)

    let security = try SecurityKeychainStore(service: "com.nebula.minutae.test.validation")
    await #expect(throws: KeychainStoreError.rejectedSecret) {
        try await security.setSecret("", for: .openai)
    }
}

@Test func keychainErrorsAndSettingsOmitSecrets() async throws {
    let secret = "sentinel-value-should-not-leak"
    let keychain = InMemoryKeychainStore()
    try await keychain.setSecret(secret, for: .openai)

    let payload = try JSONEncoder().encode(AppSettings.defaults)
    let json = try #require(String(bytes: payload, encoding: .utf8))
    #expect(!json.contains(secret))
    #expect(!json.contains("apiKey"))
    #expect(!json.contains("openai"))

    let failed = KeychainStoreError.failed(-1)
    #expect(rendered(failed).contains(secret) == false)
    #expect(rendered(KeychainStoreError.unavailable).contains(secret) == false)
}

@Test func keychainServiceNameIsCheckedBeforeAnyItemAccess() throws {
    let secret = "sentinel-value-should-not-leak"
    #expect(SecurityKeychainStore.productionService == "com.nebula.minutae")
    #expect(throws: KeychainStoreError.rejectedService) {
        try SecurityKeychainStore(service: "")
    }
    #expect(throws: KeychainStoreError.rejectedService) {
        try SecurityKeychainStore(service: "com.nebula.minutae/other")
    }
    do {
        _ = try SecurityKeychainStore(service: "\(secret)/x")
        Issue.record("Expected the service name to be rejected.")
    } catch {
        #expect(error as? KeychainStoreError == .rejectedService)
        #expect(rendered(error).contains(secret) == false)
    }
    for account in APIKeyAccount.allCases {
        #expect(account.rawValue.hasPrefix("api."))
    }
}

@Test(
    .enabled(if: KeychainAvailability.isAvailable, "Keychain is unavailable")
)
func securityKeychainRoundTripCleansUp() async throws {
    let serviceA = throwawayService("a")
    let serviceB = throwawayService("b")
    let storeA = try SecurityKeychainStore(service: serviceA)
    let storeB = try SecurityKeychainStore(service: serviceB)
    let secret = "minutae-test-secret"
    do {
        try await storeA.removeAll()
        try await storeB.removeAll()
        try await exercise(storeA, storeB, secret: secret)
    } catch KeychainStoreError.unavailable {
        try? await storeA.removeAll()
        try? await storeB.removeAll()
    } catch {
        try? await storeA.removeAll()
        try? await storeB.removeAll()
        throw error
    }
}

private func exercise(
    _ storeA: SecurityKeychainStore,
    _ storeB: SecurityKeychainStore,
    secret: String
) async throws {
    try await storeA.setSecret(secret, for: .openai)
    #expect(try await storeA.secret(for: .openai) == secret)
    #expect(try await storeA.secret(for: .anthropic) == nil)

    try await storeA.setSecret("updated-secret", for: .openai)
    try await storeB.setSecret("other-secret", for: .gemini)
    #expect(try await storeA.secret(for: .openai) == "updated-secret")
    #expect(try await storeB.secret(for: .gemini) == "other-secret")

    try await storeA.deleteSecret(for: .openai)
    #expect(try await storeA.secret(for: .openai) == nil)
    try await storeA.deleteSecret(for: .openai)
    #expect(try await storeB.secret(for: .gemini) == "other-secret")

    try await storeA.removeAll()
    #expect(try await storeB.secret(for: .gemini) == "other-secret")
    try await storeB.removeAll()
    #expect(try await storeB.secret(for: .gemini) == nil)
}

private enum KeychainAvailability {
    static let isAvailable: Bool = {
        let service = "com.nebula.minutae.test.availability.\(UUID().uuidString)"
        guard (try? ServiceName.validate(service)) != nil else { return false }
        do {
            let probe = "availability-probe"
            try KeychainDatabase.setSecret(probe, account: .openai, service: service)
            defer { try? KeychainDatabase.removeAll(service: service) }
            return try KeychainDatabase.secret(account: .openai, service: service) == probe
        } catch {
            try? KeychainDatabase.removeAll(service: service)
            return false
        }
    }()
}

private func throwawayService(_ label: String) -> String {
    "com.nebula.minutae.test.\(label).\(UUID().uuidString)"
}

private func rendered(_ error: any Error) -> String {
    let localized = (error as? any LocalizedError)?.errorDescription ?? ""
    return String(describing: error) + String(reflecting: error) + localized
}
