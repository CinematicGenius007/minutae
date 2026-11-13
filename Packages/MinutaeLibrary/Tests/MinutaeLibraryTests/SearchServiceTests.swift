import Foundation
import MinutaeCore
import Testing

@testable import MinutaeLibrary

@Test func labelledQueriesReturnTheExpectedTopHit() async throws {
    try await withLibrary { service, database in
        try Repository<Embedding>(database).save(
            Embedding(
                id: "legacy-json",
                meeting_id: "budget",
                chunk_text: "legacy",
                chunk_index: 99,
                vector: Data("[]".utf8)
            )
        )
        let budget = try await service.search(query: "quarterly budget")
        #expect(budget.first?.kind == .meeting)
        #expect(budget.first?.id == "budget")
        #expect(budget.first?.title == "Q3 budget")
        #expect(budget.first?.date == "2026-03-01T00:00:00Z")
        #expect(budget.first?.snippet.contains("<mark>") == true)
        #expect(budget.map(\.score) == budget.map(\.score).sorted(by: >))

        #expect(try await service.search(query: "tomatoes basil").first?.id == "garden")

        let keyword = try service.keywordSearch(query: "apollo launch")
        #expect(keyword.first?.kind == .dictation)
        #expect(keyword.first?.id == "launch-note")
        let hybrid = try await service.search(query: "apollo launch")
        #expect(hybrid.first?.kind == .meeting)
        #expect(hybrid.first?.id == "apollo")
        #expect(hybrid.first?.title == "Apollo")
        #expect(hybrid.first?.snippet.contains("<mark>") == true)

        let invoice = try await service.search(query: "invoice 4412")
        #expect(invoice.first?.kind == .dictation)
        #expect(invoice.first?.id == "invoice")
        #expect(invoice.first?.date == "2026-07-01T00:00:00Z")
        #expect(invoice.first?.title.contains("invoice") == true)
        #expect(invoice.first?.snippet.contains("<mark>invoice</mark>") == true)
        #expect(invoice.first?.snippet.contains("<mark>4412</mark>") == true)
    }
}

@Test func sanitisedQueriesCannotBreakFTSOrDropRows() async throws {
    try await withLibrary { service, database in
        let hostile = [
            "\"",
            "AND OR NEAR",
            "*",
            "((((",
            "apollo'; DROP TABLE meetings; --",
            "'; DROP TABLE meetings; --",
            "raw_text:apollo",
            "apollo AND launch",
            "tomatoes OR basil",
            "NEAR(apollo, launch)",
            "\"\"\"",
            "apollo\0launch",
        ]
        for query in hostile {
            _ = try service.keywordSearch(query: query)
        }
        #expect(try Repository<Meeting>(database).all().count == 3)
        #expect(try service.keywordSearch(query: "*").isEmpty)
        #expect(try service.keywordSearch(query: "apollo AND launch").isEmpty)
        #expect(try service.keywordSearch(query: "tomatoes OR basil").isEmpty)
        #expect(try service.keywordSearch(query: "NEAR(apollo, launch)").isEmpty)
        #expect(try service.keywordSearch(query: "raw_text:apollo").isEmpty)
        let prefixed = try service.keywordSearch(query: "((((apollo")
        #expect(prefixed.contains { $0.id == "apollo" && $0.kind == .meeting })
        let prefix = try service.keywordSearch(query: "apoll")
        #expect(prefix.contains { $0.id == "apollo" && $0.kind == .meeting })
        #expect(try service.keywordSearch(query: "\"apollo\"").contains { $0.id == "apollo" })
        #expect(try service.keywordSearch(query: "tomatoes basil").first?.id == "garden")
        #expect(try service.keywordSearch(query: "").isEmpty)
        #expect(try service.keywordSearch(query: "garden", limit: 0).isEmpty)
    }
}

@Test func semanticSearchIgnoresLegacyJSONAndRanksTheMatchingMeeting() async throws {
    try await withLibrary { service, database in
        try Repository<Meeting>(database).save(
            Meeting(
                id: "legacy",
                title: "Legacy",
                started_at: "2026-01-01T00:00:00Z",
                ended_at: nil,
                duration_seconds: nil,
                status: "done"
            )
        )
        try Repository<Embedding>(database).save(
            Embedding(
                id: "legacy-only",
                meeting_id: "legacy",
                chunk_text: "tomatoes from the old embedder",
                chunk_index: 0,
                vector: Data(#"{"vector":[0.1,0.2]}"#.utf8)
            )
        )
        let semantic = try await service.semanticSearch(query: "quarterly budget")
        #expect(semantic.first?.id == "budget")
        #expect(semantic.allSatisfy { $0.id != "legacy" })
        #expect((semantic.first?.score ?? 0) > 0)
        let stored = try Repository<Embedding>(database).all().filter {
            $0.meeting_id == "budget" && $0.chunk_index == 0
        }
        #expect(stored.first?.vector.count == 256 * MemoryLayout<Float>.size)
    }
}

@Test func indexMeetingReplacesChunksAndRejectsAShortVector() async throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    let database = try MinutaeDatabase(path: directory.appendingPathComponent("db.sqlite").path)
    try Repository<Meeting>(database).save(
        Meeting(
            id: "m",
            title: "M",
            started_at: "2026-01-01",
            ended_at: nil,
            duration_seconds: nil,
            status: "done"
        )
    )
    var transcript = Transcript(
        id: "t",
        meeting_id: "m",
        raw_text: "one two three four five six seven eight nine ten",
        provider: "whisper-local",
        language: nil,
        duration_ms: nil,
        created_at: "2026-01-01"
    )
    try Repository<Transcript>(database).save(transcript)
    let service = SearchService(
        database: database, embeddings: HashEmbeddingProvider(dimension: 32))
    try await service.indexMeeting("m", chunkWords: 4, overlapWords: 1)
    let rows = try Repository<Embedding>(database).all().sorted { $0.chunk_index < $1.chunk_index }
    #expect(
        rows.map(\.chunk_text) == [
            "one two three four",
            "four five six seven",
            "seven eight nine ten",
        ])
    #expect(rows.map(\.chunk_index) == [0, 1, 2])
    let decoded = VectorBlob.decode(rows[0].vector, dimension: 32)
    let fresh = try await HashEmbeddingProvider(dimension: 32).embed(text: rows[0].chunk_text)
    #expect(decoded == fresh)

    transcript.raw_text = "only zephyr"
    try Repository<Transcript>(database).save(transcript)
    try await service.indexMeeting("m")
    let replaced = try Repository<Embedding>(database).all()
    #expect(replaced.count == 1)
    #expect(replaced[0].chunk_text == "only zephyr")
    #expect(replaced[0].chunk_index == 0)

    try await service.indexMeeting("missing")
    #expect(try Repository<Embedding>(database).all().count == 1)

    let wrong = SearchService(database: database, embeddings: ShortVectorProvider())
    await #expect(throws: SearchError.dimensionMismatch(expected: 4, actual: 2)) {
        try await wrong.indexMeeting("m")
    }
}

@Test func contextualEmbeddingSmokeUsesOnDeviceAssetsWhenPresent() async throws {
    guard let provider = ContextualEmbeddingProvider() else {
        print("UNVERIFIED: NLContextualEmbedding(language:) returned nil; smoke test skipped")
        return
    }
    guard await provider.assetsAvailable() else {
        print("UNVERIFIED: NLContextualEmbedding assets are not installed; smoke test skipped")
        return
    }
    let vector = try await provider.embed(text: "The quarterly budget review")
    #expect(vector.count == provider.dimension)
    #expect(vector.contains { $0 != 0 })
    let again = try await provider.embed(text: "The quarterly budget review")
    #expect(abs(VectorMath.cosineSimilarity(vector, again) - 1) < 0.001)
}

private struct ShortVectorProvider: EmbeddingProvider {
    let dimension = 4
    func embed(text: String) async throws -> [Float] { [1, 0] }
}

private func withLibrary(
    _ body: (SearchService, MinutaeDatabase) async throws -> Void
) async throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    let database = try MinutaeDatabase(path: directory.appendingPathComponent("db.sqlite").path)
    try seed(database)
    let service = SearchService(
        database: database, embeddings: HashEmbeddingProvider(dimension: 256))
    for id in ["budget", "garden", "apollo"] {
        try await service.indexMeeting(id)
    }
    try await body(service, database)
}

private struct Sample {
    var id: String
    var title: String
    var date: String
    var text: String
}

private let librarySamples = [
    Sample(
        id: "budget",
        title: "Q3 budget",
        date: "2026-03-01T00:00:00Z",
        text: "The quarterly budget review covered hiring costs and the forecast."
    ),
    Sample(
        id: "garden",
        title: "Garden",
        date: "2026-04-02T00:00:00Z",
        text: "The garden tomatoes need water and the basil looks wilted."
    ),
    Sample(
        id: "apollo",
        title: "Apollo",
        date: "2026-05-03T00:00:00Z",
        text: "The apollo launch window depends on the weather at the cape."
    ),
]

private func seed(_ database: MinutaeDatabase) throws {
    for sample in librarySamples {
        try saveMeeting(database, sample)
    }
    try saveDictation(
        database,
        id: "launch-note",
        date: "2026-12-01T00:00:00Z",
        text: "Check the apollo launch weather tonight.",
        appName: "Notes"
    )
    try saveDictation(
        database,
        id: "invoice",
        date: "2026-07-01T00:00:00Z",
        text: "Send the invoice number 4412 to the vendor.",
        appName: nil
    )
}

private func saveMeeting(_ database: MinutaeDatabase, _ sample: Sample) throws {
    try Repository<Meeting>(database).save(
        Meeting(
            id: sample.id,
            title: sample.title,
            started_at: sample.date,
            ended_at: nil,
            duration_seconds: nil,
            status: "done"
        )
    )
    try Repository<Transcript>(database).save(
        Transcript(
            id: "t-\(sample.id)",
            meeting_id: sample.id,
            raw_text: sample.text,
            provider: "whisper-local",
            language: "en",
            duration_ms: nil,
            created_at: sample.date
        )
    )
}

private func saveDictation(
    _ database: MinutaeDatabase,
    id: String,
    date: String,
    text: String,
    appName: String?
) throws {
    try Repository<Dictation>(database).save(
        Dictation(
            id: id,
            started_at: date,
            duration_ms: nil,
            target_app_bundle_id: nil,
            target_app_name: appName,
            raw_text: text,
            final_text: text,
            language: nil,
            insert_method: nil,
            status: "done",
            audio_path: nil,
            error: nil,
            created_at: date
        )
    )
}
