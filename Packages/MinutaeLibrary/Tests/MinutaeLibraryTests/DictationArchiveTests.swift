import Foundation
import MinutaeCore
import Testing

@testable import MinutaeLibrary

struct DictationArchiveTests {
    struct Fixture {
        var root: URL
        var audio: URL
        var database: MinutaeDatabase
        var archive: DatabaseDictationArchive

        init() throws {
            root = FileManager.default.temporaryDirectory
                .appendingPathComponent("minutae-archive-\(UUID().uuidString)")
            audio = root.appendingPathComponent("dictations", isDirectory: true)
            try FileManager.default.createDirectory(at: audio, withIntermediateDirectories: true)
            let path = root.appendingPathComponent("db.sqlite").path
            database = try MinutaeDatabase(path: path)
            archive = DatabaseDictationArchive(
                database: database,
                audioRoot: audio,
                now: { Date(timeIntervalSince1970: 1_794_580_000) }
            )
        }

        @discardableResult
        func add(
            _ id: String,
            started: String = "2026-11-13T14:41:00Z",
            status: String = "done",
            wav: Bool = true
        ) throws -> Dictation {
            var path: String?
            if wav {
                let url = audio.appendingPathComponent("\(id).wav")
                try Data("RIFF".utf8).write(to: url)
                path = url.path
            }
            let row = Dictation(
                id: id, started_at: started, duration_ms: 6_000,
                target_app_bundle_id: "com.tinyspeck.slackmacgap", target_app_name: "Slack",
                raw_text: "remind me", final_text: "Remind me.", language: "en",
                insert_method: "pasteboard", status: status, audio_path: path, error: nil,
                created_at: started, speech_model: "small")
            try Repository<Dictation>(database).save(row)
            return row
        }

        func cleanup() { try? FileManager.default.removeItem(at: root) }
    }

    @Test func editKeepsOriginalTextAndAudio() throws {
        let fixture = try Fixture()
        defer { fixture.cleanup() }
        let row = try fixture.add("a")
        let saved = try #require(try fixture.archive.saveEdit(id: "a", text: "Remind me now."))
        #expect(saved.edited_text == "Remind me now.")
        #expect(saved.edited_at == "2026-11-13T14:26:40Z")
        #expect(saved.final_text == "Remind me.")
        #expect(saved.raw_text == "remind me")
        #expect(saved.displayText == "Remind me now.")
        #expect(FileManager.default.fileExists(atPath: try #require(row.audio_path)))

        let reverted = try #require(try fixture.archive.saveEdit(id: "a", text: "Remind me."))
        #expect(reverted.edited_text == nil)
        #expect(reverted.edited_at == nil)
    }

    @Test func editAndDeleteRefuseRowsInProgress() throws {
        let fixture = try Fixture()
        defer { fixture.cleanup() }
        try fixture.add("live", status: "processing")
        #expect(throws: DictationArchiveError.inProgress) {
            try fixture.archive.saveEdit(id: "live", text: "x")
        }
        #expect(throws: DictationArchiveError.inProgress) {
            try fixture.archive.delete(id: "live")
        }
        #expect(throws: DictationArchiveError.notFound) {
            try fixture.archive.delete(id: "missing")
        }
    }

    @Test func deleteRemovesRowAudioTagsAndJobs() throws {
        let fixture = try Fixture()
        defer { fixture.cleanup() }
        let row = try fixture.add("a")
        try fixture.add("b")
        try fixture.database.pool.write { connection in
            try connection.execute(sql: "INSERT INTO tags VALUES ('t', 'Work', NULL, '2026-01-01')")
            try connection.execute(sql: "INSERT INTO item_tags VALUES ('t', 'dictation', 'a')")
        }
        let result = try fixture.archive.delete(id: "a")
        #expect(result == DictationDeletion(deletedRows: 1, deletedAudioFiles: 1))
        #expect(try fixture.archive.dictation(id: "a") == nil)
        #expect(try fixture.archive.dictation(id: "b") != nil)
        #expect(!FileManager.default.fileExists(atPath: try #require(row.audio_path)))
        #expect(try Repository<ItemTag>(fixture.database).all().isEmpty)
    }

    @Test func deleteAllKeepsRowsInProgressAndReportsMissingAudio() throws {
        let fixture = try Fixture()
        defer { fixture.cleanup() }
        try fixture.add("a")
        try fixture.add("b", wav: false)
        try fixture.add("c", status: "listening")
        let result = try fixture.archive.deleteAll()
        #expect(result.deletedRows == 2)
        #expect(result.deletedAudioFiles == 1)
        #expect(result.audioFailures.isEmpty)
        #expect(try Repository<Dictation>(fixture.database).all().map(\.id) == ["c"])
    }

    @Test func audioOutsideTheRootIsNeverDeleted() throws {
        let fixture = try Fixture()
        defer { fixture.cleanup() }
        let outside = fixture.root.appendingPathComponent("keep.wav")
        try Data("RIFF".utf8).write(to: outside)
        var row = try fixture.add("a", wav: false)
        row.audio_path = outside.path
        try Repository<Dictation>(fixture.database).save(row)
        let result = try fixture.archive.delete(id: "a")
        #expect(result.audioFailures == [outside.path])
        #expect(FileManager.default.fileExists(atPath: outside.path))
        #expect(
            !DictationAudioLocation.isInside(
                fixture.audio.appendingPathComponent("../keep.wav"), root: fixture.audio))
    }

    @Test func observeRecentIsNewestFirstAndFollowsChanges() async throws {
        let fixture = try Fixture()
        defer { fixture.cleanup() }
        try fixture.add("old", started: "2026-11-13T09:00:00Z")
        try fixture.add("new", started: "2026-11-13T15:00:00Z")
        var iterator = fixture.archive.observeRecent(limit: 10).makeAsyncIterator()
        let first = try await iterator.next()
        #expect(first?.map(\.id) == ["new", "old"])
        _ = try fixture.archive.delete(id: "new")
        let second = try await iterator.next()
        #expect(second?.map(\.id) == ["old"])
    }

    @Test func markdownHasFrontmatterAndTheEditedText() throws {
        var row = Dictation(
            id: "a", started_at: "2026-11-13T14:41:00Z", duration_ms: 6_000,
            target_app_bundle_id: nil, target_app_name: "Slack \"beta\"",
            raw_text: "raw", final_text: "Final.", language: "en", insert_method: "pasteboard",
            status: "done", audio_path: "/tmp/a.wav", error: nil,
            created_at: "2026-11-13T14:41:00Z", speech_model: "small")
        row.edited_text = "Edited.\n"
        let markdown = DictationMarkdown.render(row)
        #expect(markdown.hasPrefix("---\ntype: dictation\n"))
        #expect(markdown.contains("app: \"Slack \\\"beta\\\"\"\n"))
        #expect(markdown.contains("model: \"whisper small\"\n"))
        #expect(markdown.contains("duration_ms: 6000\n"))
        #expect(markdown.hasSuffix("---\n\nEdited.\n"))
        let name = DictationMarkdown.suggestedFileName(
            for: row, extension: "md", timeZone: try #require(TimeZone(secondsFromGMT: 0)))
        #expect(name == "dictation-2026-11-13-1441.md")
    }

    @Test func searchUsesTheEditedText() throws {
        let fixture = try Fixture()
        defer { fixture.cleanup() }
        try fixture.add("a")
        try fixture.archive.saveEdit(id: "a", text: "Ping the kumquat team.")
        let service = SearchService(
            database: fixture.database, embeddings: HashEmbeddingProvider())
        let hits = try service.keywordSearch(query: "kumquat")
        #expect(hits.map(\.id) == ["a"])
    }
}
