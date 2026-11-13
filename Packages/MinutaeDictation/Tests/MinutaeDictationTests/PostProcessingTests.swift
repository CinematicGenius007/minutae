import Foundation
import MinutaeCore
import Testing

@testable import MinutaeDictation

struct GoldenEntry: Decodable {
    var id: String
    var term: String
    var replacement: String
    var kind: String
    var scope: String?
    var enabled: Bool?
}

struct GoldenCase: Decodable {
    var name: String
    var input: String
    var expected: String
    var bundleID: String?
    var entries: [GoldenEntry]?
}

struct GoldenFile: Decodable {
    var cases: [GoldenCase]
}

@Test func goldenFileMatchesFixtures() async throws {
    let url = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .appendingPathComponent("Fixtures/postprocess.json")
    let file = try JSONDecoder().decode(GoldenFile.self, from: Data(contentsOf: url))
    #expect(!file.cases.isEmpty)
    for item in file.cases {
        let stored = (item.entries ?? []).map { entry in
            DictionaryEntry(
                id: entry.id,
                term: entry.term,
                replacement: entry.replacement,
                kind: entry.kind,
                scope_bundle_id: entry.scope,
                enabled: entry.enabled ?? true,
                created_at: "2026-01-01T00:00:00Z"
            )
        }
        let processor = LocalPostProcessor(dictionary: InMemoryDictionaryRepository(stored))
        let actual = await processor.process(text: item.input, bundleID: item.bundleID)
        #expect(actual == item.expected, "\(item.name) produced \(actual)")
    }
}

@Test func perAppScopeOverridesGlobalAndFallsBack() async {
    let slack = "com.tinyspeck.slackmacgap"
    let notes = "com.apple.Notes"
    let entries = [
        dictionaryEntry("global", "brb", "be right back", kind: "word", scope: nil),
        dictionaryEntry("slack", "brb", "back shortly", kind: "word", scope: slack),
        dictionaryEntry("notes-sig", "sig", "From Notes", kind: "snippet", scope: notes),
    ]
    let processor = LocalPostProcessor(dictionary: InMemoryDictionaryRepository(entries))

    let inSlack = await processor.process(text: "brb sig", bundleID: slack)
    #expect(inSlack == "Back shortly sig.")

    let inNotes = await processor.process(text: "brb sig", bundleID: notes)
    #expect(inNotes == "Be right back From Notes.")

    let inTextEdit = await processor.process(text: "brb sig", bundleID: "com.apple.TextEdit")
    #expect(inTextEdit == "Be right back sig.")

    let unknownApp = await processor.process(text: "brb")
    #expect(unknownApp == "Be right back.")
}

@Test func llmCleanupStaysOffUnlessEnabled() async {
    let cleaner = FakeLLMTextCleanup(result: .success("Rewritten."))
    let off = LocalPostProcessor(dictionary: InMemoryDictionaryRepository(), llm: cleaner)
    let untouched = await off.process(text: "um hello", bundleID: nil)
    #expect(untouched == "Hello.")
    #expect(await cleaner.inputs.isEmpty)

    let enabled = LocalPostProcessor(
        dictionary: InMemoryDictionaryRepository(),
        llm: cleaner,
        llmEnabled: true
    )
    let rewritten = await enabled.process(text: "um hello", bundleID: "com.apple.TextEdit")
    #expect(rewritten == "Rewritten.")
    #expect(await cleaner.inputs == ["Hello."])

    let closure = ClosureLLMTextCleanup { text in
        "(\(text))"
    }
    let wrapped = LocalPostProcessor(
        dictionary: InMemoryDictionaryRepository(),
        llm: closure,
        llmEnabled: true
    )
    let wrappedText = await wrapped.process(text: "hello")
    #expect(wrappedText == "(Hello.)")
}

@Test func llmFailureOrBlankKeepsLocalText() async {
    let failing = FakeLLMTextCleanup(result: .failure(TextCleanupFailure(message: "down")))
    let processor = LocalPostProcessor(
        dictionary: InMemoryDictionaryRepository(),
        llm: failing,
        llmEnabled: true
    )
    let kept = await processor.process(text: "hello")
    #expect(kept == "Hello.")

    let blank = FakeLLMTextCleanup(result: .success("  \n"))
    let blankProcessor = LocalPostProcessor(
        dictionary: InMemoryDictionaryRepository(),
        llm: blank,
        llmEnabled: true
    )
    let stillLocal = await blankProcessor.process(text: "hello")
    #expect(stillLocal == "Hello.")
}

@Test func missingCleanerDoesNotEnableLLM() async {
    let processor = LocalPostProcessor(llmEnabled: true)
    let text = await processor.process(text: "um hello there")
    #expect(text == "Hello there.")
}

@Test func fakePostProcessorRecordsTheRequest() async {
    let fake = FakePostProcessor(result: "scripted")
    let first = await fake.process(text: "raw", bundleID: "com.apple.Notes")
    let second = await fake.process(text: "other")
    #expect(first == "scripted")
    #expect(second == "scripted")
    #expect(await fake.texts == ["raw", "other"])
    #expect(await fake.bundleIDs == ["com.apple.Notes", nil])
}

private func dictionaryEntry(
    _ id: String,
    _ term: String,
    _ replacement: String,
    kind: String,
    scope: String?
) -> DictionaryEntry {
    DictionaryEntry(
        id: id,
        term: term,
        replacement: replacement,
        kind: kind,
        scope_bundle_id: scope,
        enabled: true,
        created_at: "2026-01-01T00:00:00Z"
    )
}
