import Foundation
import GRDB
import Testing
@testable import MinutaeCore

@Test func legacyMigrationPreservesData() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    let path = directory.appendingPathComponent("fixture.sqlite").path
    let legacy = try DatabaseQueue(path: path)
    try legacy.write { db in
        for name in ["0001_init", "0002_fts_triggers"] {
            let url = try #require(Bundle.module.url(forResource: name, withExtension: "sql"))
            try db.execute(sql: String(contentsOf: url, encoding: .utf8))
        }
        try db.execute(sql: """
            CREATE TABLE __drizzle_migrations (id INTEGER PRIMARY KEY, hash TEXT, created_at INTEGER);
            INSERT INTO __drizzle_migrations VALUES (1, 'fake', 1);
            INSERT INTO meetings (id, started_at) VALUES ('m', '2026-01-01');
            INSERT INTO transcripts (id, meeting_id, raw_text, created_at) VALUES ('t', 'm', 'hello', '2026-01-01');
            INSERT INTO segments VALUES ('s', 't', 0, 100, 'hello', NULL);
            INSERT INTO summaries VALUES ('u', 'm', 'summary', 'model', 'provider', '2026-01-01');
            """)
    }
    let tables = ["meetings", "recordings", "transcripts", "segments", "summaries", "action_items", "embeddings", "api_usage_log", "settings", "__drizzle_migrations"]
    let counts = try legacy.read { db in try tables.map { try Int.fetchOne(db, sql: "SELECT count(*) FROM \($0)")! } }
    let database = try MinutaeDatabase(path: path)
    #expect(try Repository<Meeting>(database).newestFirst().first?.id == "m")
    #expect(try Repository<Segment>(database).forTranscript("t").first?.text == "hello")
    #expect(try Repository<Summary>(database).all().first?.markdown_content == "summary")
    let after = try database.pool.read { db in try tables.map { try Int.fetchOne(db, sql: "SELECT count(*) FROM \($0)")! } }
    #expect(counts == after)
    try database.pool.read { db throws in
        #expect(try String.fetchOne(db, sql: "PRAGMA journal_mode") == "wal")
        #expect(try Int.fetchOne(db, sql: "PRAGMA foreign_keys") == 1)
        #expect((try Int.fetchOne(db, sql: "PRAGMA busy_timeout") ?? 0) > 0)
    }
    try database.pool.write { db in
        let timeout = try Int.fetchOne(db, sql: "PRAGMA busy_timeout")
        #expect(timeout == 5000)
    }
    _ = try MinutaeDatabase(path: path)
    try database.pool.write { db in
        try db.execute(sql: "DELETE FROM grdb_migrations WHERE identifier = 'v2'")
    }
    _ = try MinutaeDatabase(path: path)
    #expect(try Repository<Meeting>(database).all().count == 1)
}

@Test func freshDatabaseCRUDAndConstraints() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: directory) }
    let database = try MinutaeDatabase(path: directory.appendingPathComponent("db.sqlite").path)
    let meetings = Repository<Meeting>(database)
    let meeting = Meeting(id: "m", title: "First", started_at: "2026-01-01", ended_at: nil, duration_seconds: nil, status: "done")
    try meetings.save(meeting)
    #expect(try meetings.find("m")?.title == "First")
    try database.pool.write { db in
        try db.execute(sql: "INSERT INTO tags VALUES ('tag', 'Work', NULL, '2026-01-01')")
        try db.execute(sql: "INSERT INTO item_tags VALUES ('tag', 'meeting', 'm')")
        try db.execute(sql: "INSERT INTO jobs (id, kind, subject_type, subject_id, created_at, updated_at) VALUES ('j', 'transcribe', 'meeting', 'm', '2026-01-01', '2026-01-01')")
    }
    #expect(try Repository<Job>(database).byState("queued").count == 1)
    #expect(throws: (any Error).self) {
        try database.pool.write { db in
            try db.execute(sql: "INSERT INTO item_tags VALUES ('missing', 'meeting', 'm')")
        }
    }
    #expect(try meetings.delete(meeting))
    #expect(try Repository<Job>(database).all().isEmpty)
    #expect(try Repository<ItemTag>(database).all().isEmpty)
}

@Test func v4AddsDictationEditColumnsAndKeepsRows() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: directory) }
    let path = directory.appendingPathComponent("db.sqlite").path
    let database = try MinutaeDatabase(path: path)
    try database.pool.write { connection in
        try connection.execute(sql: "DELETE FROM grdb_migrations WHERE identifier = 'v4'")
        try connection.execute(sql: "ALTER TABLE dictations DROP COLUMN edited_text")
        try connection.execute(sql: "ALTER TABLE dictations DROP COLUMN edited_at")
        try connection.execute(sql: "ALTER TABLE dictations DROP COLUMN speech_model")
        try connection.execute(
            sql: """
                INSERT INTO dictations (id, started_at, raw_text, final_text, status, created_at)
                VALUES ('d', '2026-11-13T14:41:00Z', 'raw', 'Final.', 'done', '2026-11-13T14:41:00Z')
                """)
    }
    let migrated = try MinutaeDatabase(path: path)
    let columns = try migrated.pool.read { connection in
        try Set(connection.columns(in: "dictations").map(\.name))
    }
    #expect(columns.isSuperset(of: ["edited_text", "edited_at", "speech_model"]))
    var row = try #require(try Repository<Dictation>(migrated).find("d"))
    #expect(row.edited_text == nil)
    #expect(row.displayText == "Final.")
    row.edited_text = "Edited."
    try Repository<Dictation>(migrated).save(row)
    let saved = try #require(try Repository<Dictation>(migrated).find("d"))
    #expect(saved.displayText == "Edited.")
    #expect(saved.final_text == "Final.")
    #expect(saved.raw_text == "raw")
}
