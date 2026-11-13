import Foundation
import Testing

@testable import MinutaeCore

@Test func settingsDefaultsMatchArchitecture() throws {
    let settings = AppSettings.defaults
    #expect(settings.dictationHotkey == .default)
    #expect(settings.dictationHotkey.mode == .hold)
    #expect(settings.dictationHotkey.chord == .optionSpace)
    #expect(settings.dictationHotkey.chord.keyCode == HotkeyChord.spaceKeyCode)
    #expect(settings.dictationHotkey.chord.modifiers == .option)
    #expect(settings.keepDictationAudio)
    #expect(settings.audioRetention == .never)
    #expect(settings.codexModel == nil)
    #expect(settings.transportFallbackOrder == [.codexCLI, .grokCLI, .ollama])
    #expect(!settings.transportFallbackOrder.contains(.api))
    #expect(settings.apiTransportEnabled == false)
    #expect(settings.whisperModelName == "base")
    #expect(settings.launchAtLogin == false)
    #expect(settings.autoDetectMeetings)
    #expect(settings.promptDelaySeconds == 5)
    #expect(SettingsSanitizer.promptDelayRange == 1...30)
    #expect(settings.recordingsPath.hasSuffix("/Library/Minutae/recordings"))
    #expect(settings.summariesPath.hasSuffix("/Library/Minutae/summaries"))
    #expect(settings.dictationsPath.hasSuffix("/Library/Minutae/dictations"))
    #expect(settings.recordingsPath == MinutaeDirectories.recordings.path)
    try settings.validate()
}

@Test func settingsRoundTripThroughUserDefaults() async throws {
    let (defaults, suiteName) = try throwawayDefaults()
    defer { defaults.removePersistentDomain(forName: suiteName) }
    let store = UserDefaultsSettingsStore(userDefaults: defaults)
    let secret = "sentinel-value-should-not-leak"
    let keychain = InMemoryKeychainStore()
    try await keychain.setSecret(secret, for: .anthropic)

    var settings = AppSettings.defaults
    settings.dictationHotkey = DictationHotkey(
        mode: .toggle,
        chord: HotkeyChord(keyCode: 40, modifiers: [.command, .shift])
    )
    settings.keepDictationAudio = false
    settings.audioRetention = .days(count: 14)
    settings.codexModel = "gpt-5.4"
    settings.transportFallbackOrder = [.ollama, .grokCLI, .api]
    settings.apiTransportEnabled = true
    settings.recordingsPath = "/tmp/minutae-recordings"
    settings.summariesPath = "/tmp/minutae-summaries"
    settings.dictationsPath = "/tmp/minutae-dictations"
    settings.whisperModelName = "large-v3"
    settings.launchAtLogin = true
    settings.autoDetectMeetings = false
    settings.promptDelaySeconds = 12

    try await store.save(settings)
    let loaded = try await store.load()
    #expect(loaded == settings)

    let reopened = UserDefaultsSettingsStore(userDefaults: defaults)
    #expect(try await reopened.load() == settings)

    let payload = try #require(defaults.data(forKey: UserDefaultsSettingsStore.storageKey))
    let text = try #require(String(bytes: payload, encoding: .utf8))
    #expect(!text.contains(secret))
    #expect(!text.contains("apiKey"))
    #expect(!text.contains("anthropic"))
}

@Test func missingSettingsReturnDefaultsWithoutWriting() async throws {
    let (defaults, suiteName) = try throwawayDefaults()
    defer { defaults.removePersistentDomain(forName: suiteName) }
    let store = UserDefaultsSettingsStore(userDefaults: defaults)
    #expect(try await store.load() == .defaults)
    #expect(defaults.data(forKey: UserDefaultsSettingsStore.storageKey) == nil)
}

@Test func settingsValidationRejectsBadModelNamesAndPaths() throws {
    let badModels =
        ["", " ", "../x", "a/b", "a\\b"]
        + ["..", "a..b", ".hidden", "base\nbad", "gpt\u{0000}"]
    for name in badModels {
        var settings = AppSettings.defaults
        settings.whisperModelName = name
        #expect(throws: SettingsValidationError.self) {
            try settings.validate()
        }
        settings = AppSettings.defaults
        settings.codexModel = name
        #expect(throws: SettingsValidationError.self) {
            try settings.validate()
        }
    }

    let accepted = ["base", "large-v3", "ggml-base.en.bin", "llama3.2:3b", "gpt-5.4"]
    for name in accepted {
        try SettingsSanitizer.validateModelName(name)
    }

    let badPaths =
        ["", "relative/path", "../x", "/tmp/../etc"]
        + ["/tmp/foo/../../etc/passwd", "/tmp/foo/..", "a\\b"]
        + ["/tmp/bad\nname", "/tmp/\u{0000}x", "/tmp/%2e%2e/etc"]
        + ["/tmp/%252e%252e/etc", "/"]
    for path in badPaths {
        #expect(throws: SettingsValidationError.self) {
            try SettingsSanitizer.validateStoragePath(path)
        }
    }
    try SettingsSanitizer.validateStoragePath("/tmp/minutae recordings")
    try SettingsSanitizer.validateStoragePath(MinutaeDirectories.recordings.path)
}

@Test func validationErrorsOmitRejectedText() {
    let secret = "sentinel-value-should-not-leak"
    var settings = AppSettings.defaults
    settings.codexModel = "../\(secret)"
    settings.recordingsPath = "/tmp/\(secret)/../etc"
    do {
        try settings.validate()
        Issue.record("Expected validation to fail.")
    } catch {
        #expect(rendered(error).contains(secret) == false)
    }
}

@Test func storeRejectsInvalidSettingsWithoutPersistingOrNotifying() async throws {
    let (defaults, suiteName) = try throwawayDefaults()
    defer { defaults.removePersistentDomain(forName: suiteName) }
    let store = UserDefaultsSettingsStore(userDefaults: defaults)
    let stream = await store.changes()
    var iterator = stream.makeAsyncIterator()

    var bad = AppSettings.defaults
    bad.recordingsPath = "/tmp/../etc"
    await #expect(throws: SettingsStoreError.invalid(.pathTraversal)) {
        try await store.save(bad)
    }
    #expect(defaults.data(forKey: UserDefaultsSettingsStore.storageKey) == nil)
    #expect(try await store.load() == .defaults)

    bad.recordingsPath = AppSettings.defaults.recordingsPath
    bad.audioRetention = .days(count: 0)
    await #expect(throws: SettingsStoreError.invalid(.invalidRetention)) {
        try await store.save(bad)
    }
    bad.audioRetention = .never
    bad.promptDelaySeconds = 0
    await #expect(throws: SettingsStoreError.invalid(.invalidPromptDelay)) {
        try await store.save(bad)
    }

    var good = AppSettings.defaults
    good.launchAtLogin = true
    try await store.save(good)
    let next = await iterator.next()
    #expect(next == good)
}

@Test func corruptSettingsAreUnreadableAndRedacted() async throws {
    let (defaults, suiteName) = try throwawayDefaults()
    defer { defaults.removePersistentDomain(forName: suiteName) }
    let secret = "sentinel-value-should-not-leak"
    defaults.set(Data("not-json-\(secret)".utf8), forKey: UserDefaultsSettingsStore.storageKey)
    let store = UserDefaultsSettingsStore(userDefaults: defaults)
    do {
        _ = try await store.load()
        Issue.record("Expected unreadable settings.")
    } catch {
        #expect(error as? SettingsStoreError == .unreadable)
        #expect(rendered(error).contains(secret) == false)
    }
}

@Test func inMemoryStoreNotifiesEverySubscriber() async throws {
    let store = InMemorySettingsStore()
    let first = await store.changes()
    let second = await store.changes()
    var firstIterator = first.makeAsyncIterator()
    var secondIterator = second.makeAsyncIterator()

    var updated = AppSettings.defaults
    updated.autoDetectMeetings = false
    updated.whisperModelName = "small"
    try await store.save(updated)

    let left = await firstIterator.next()
    let right = await secondIterator.next()
    #expect(left == updated)
    #expect(right == updated)
    #expect(try await store.load() == updated)
}

@Test func partialSettingsJSONKeepsDefaults() async throws {
    let (defaults, suiteName) = try throwawayDefaults()
    defer { defaults.removePersistentDomain(forName: suiteName) }
    let json = Data(#"{"launchAtLogin":true,"whisperModelName":"tiny"}"#.utf8)
    defaults.set(json, forKey: UserDefaultsSettingsStore.storageKey)
    let store = UserDefaultsSettingsStore(userDefaults: defaults)
    let loaded = try await store.load()
    #expect(loaded.launchAtLogin)
    #expect(loaded.whisperModelName == "tiny")
    #expect(loaded.codexModel == nil)
    #expect(loaded.transportFallbackOrder == TransportID.defaultFallbackOrder)
    #expect(loaded.keepDictationAudio)
}

private func throwawayDefaults() throws -> (UserDefaults, String) {
    let suiteName = "com.nebula.minutae.tests.\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suiteName))
    return (defaults, suiteName)
}

private func rendered(_ error: any Error) -> String {
    let localized = (error as? any LocalizedError)?.errorDescription ?? ""
    return String(describing: error) + String(reflecting: error) + localized
}
