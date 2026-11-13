import Foundation
import MinutaeCore
import Testing

@testable import MinutaeDictation

@Test func databaseDictionaryRepositoryFiltersScopeOnTempDatabase() async throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: directory) }
    let database = try MinutaeDatabase(path: directory.appendingPathComponent("db.sqlite").path)
    let slack = "com.tinyspeck.slackmacgap"
    let notes = "com.apple.Notes"
    try seed(database, slack: slack, notes: notes)

    let repository = DatabaseDictionaryRepository(database)
    let forSlack = try repository.entries(forBundleID: slack).map(\.id).sorted()
    #expect(forSlack == ["global-word", "slack-snippet", "slack-word"])

    let forNotes = try repository.entries(forBundleID: notes).map(\.id).sorted()
    #expect(forNotes == ["global-word", "notes-word"])

    let globalOnly = try repository.entries(forBundleID: nil).map(\.id)
    #expect(globalOnly == ["global-word"])

    let processor = LocalPostProcessor(dictionary: repository)
    let slackText = await processor.process(text: "um brb now sig", bundleID: slack)
    #expect(slackText == "Back shortly now Thanks,\nAlex.")

    let notesText = await processor.process(text: "brb", bundleID: notes)
    #expect(notesText == "Notes only.")

    let elsewhere = await processor.process(text: "brb lol", bundleID: "com.apple.TextEdit")
    #expect(elsewhere == "Be right back lol.")

    let disabledOnly = await processor.process(text: "lol", bundleID: nil)
    #expect(disabledOnly == "Lol.")
}

private func seed(_ database: MinutaeDatabase, slack: String, notes: String) throws {
    let store = Repository<DictionaryEntry>(database)
    let stamp = "2026-01-01T00:00:00Z"
    try store.save(
        DictionaryEntry(
            id: "global-word", term: "brb", replacement: "be right back", kind: "word",
            scope_bundle_id: nil, enabled: true, created_at: stamp
        )
    )
    try store.save(
        DictionaryEntry(
            id: "slack-word", term: "brb", replacement: "back shortly", kind: "word",
            scope_bundle_id: slack, enabled: true, created_at: stamp
        )
    )
    try store.save(
        DictionaryEntry(
            id: "slack-snippet", term: "sig", replacement: "Thanks,\nAlex", kind: "snippet",
            scope_bundle_id: slack, enabled: true, created_at: stamp
        )
    )
    try store.save(
        DictionaryEntry(
            id: "disabled", term: "lol", replacement: "laughing out loud", kind: "word",
            scope_bundle_id: nil, enabled: false, created_at: stamp
        )
    )
    try store.save(
        DictionaryEntry(
            id: "notes-word", term: "brb", replacement: "notes only", kind: "word",
            scope_bundle_id: notes, enabled: true, created_at: stamp
        )
    )
}
