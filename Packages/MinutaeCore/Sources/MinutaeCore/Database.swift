import Foundation
import GRDB

public struct MinutaeDatabase: Sendable {
    public let pool: DatabasePool

    public init(path: String? = nil) throws {
        let path = path ?? FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Minutae/minutae.db").path
        try FileManager.default.createDirectory(
            at: URL(fileURLWithPath: path).deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        var configuration = Configuration()
        configuration.busyMode = .timeout(5)
        configuration.foreignKeysEnabled = true
        pool = try DatabasePool(path: path, configuration: configuration)
        var migrator = DatabaseMigrator()
        migrator.registerMigration("v1") { db in
            let url = Bundle.module.url(forResource: "v1", withExtension: "sql")!
            try db.execute(sql: String(contentsOf: url, encoding: .utf8))
        }
        migrator.registerMigration("v2") { db in
            let url = Bundle.module.url(forResource: "v2", withExtension: "sql")!
            try db.execute(sql: String(contentsOf: url, encoding: .utf8))
            let columns = try Set(db.columns(in: "api_usage_log").map(\.name))
            for (name, type) in [("transport", "TEXT"), ("duration_ms", "INTEGER"), ("error_class", "TEXT")] where !columns.contains(name) {
                try db.execute(sql: "ALTER TABLE api_usage_log ADD COLUMN \(name) \(type)")
            }
        }
        // v1 declared token counts and cost_usd NOT NULL. Unknown tokens and non-api cost are NULL.
        migrator.registerMigration("v3") { db in
            try makeUsageCostsNullable(db)
        }
        // Dictation edits are stored beside the original. `raw_text` and `final_text` are never
        // rewritten by an edit. `speech_model` records the Whisper model that produced the text.
        migrator.registerMigration("v4") { connection in
            try addDictationEditColumns(connection)
        }
        try migrator.migrate(pool)
    }
}

private func addDictationEditColumns(_ connection: Database) throws {
    let columns = try Set(connection.columns(in: "dictations").map(\.name))
    for name in ["edited_text", "edited_at", "speech_model"] where !columns.contains(name) {
        try connection.execute(sql: "ALTER TABLE dictations ADD COLUMN \(name) TEXT")
    }
}

private func makeUsageCostsNullable(_ db: Database) throws {
    try db.execute(sql: """
        CREATE TABLE api_usage_log_v3 (
          id            TEXT PRIMARY KEY NOT NULL,
          meeting_id    TEXT REFERENCES meetings(id) ON DELETE SET NULL,
          provider      TEXT NOT NULL,
          model         TEXT NOT NULL,
          endpoint      TEXT NOT NULL,
          input_tokens  INTEGER,
          output_tokens INTEGER,
          cost_usd      REAL,
          created_at    TEXT NOT NULL,
          transport     TEXT,
          duration_ms   INTEGER,
          error_class   TEXT
        )
        """)
    try db.execute(sql: """
        INSERT INTO api_usage_log_v3 (
          id, meeting_id, provider, model, endpoint,
          input_tokens, output_tokens, cost_usd, created_at,
          transport, duration_ms, error_class
        )
        SELECT
          id,
          CASE
            WHEN meeting_id IS NOT NULL AND EXISTS (
              SELECT 1 FROM meetings WHERE meetings.id = api_usage_log.meeting_id
            ) THEN meeting_id
            ELSE NULL
          END,
          provider, model, endpoint,
          input_tokens, output_tokens, cost_usd, created_at,
          transport, duration_ms, error_class
        FROM api_usage_log
        """)
    try db.execute(sql: "DROP TABLE api_usage_log")
    try db.execute(sql: "ALTER TABLE api_usage_log_v3 RENAME TO api_usage_log")
    try db.execute(sql: """
        CREATE INDEX IF NOT EXISTS idx_api_usage_created_at
        ON api_usage_log(created_at DESC)
        """)
}

public struct Repository<Record: FetchableRecord & PersistableRecord & TableRecord & Sendable>: Sendable {
    private let pool: DatabasePool
    public init(_ database: MinutaeDatabase) { pool = database.pool }
    public func all() throws -> [Record] { try pool.read { try Record.fetchAll($0) } }
    public func find(_ key: String) throws -> Record? {
        try pool.read { try Record.fetchOne($0, key: key) }
    }
    public func save(_ record: Record) throws { try pool.write { try record.save($0) } }
    @discardableResult public func delete(_ record: Record) throws -> Bool {
        try pool.write { db in
            let subject: (String, String)?
            if let meeting = record as? Meeting { subject = ("meeting", meeting.id) }
            else if let dictation = record as? Dictation { subject = ("dictation", dictation.id) }
            else { subject = nil }
            let deleted = try record.delete(db)
            if deleted, let subject {
                try db.execute(sql: "DELETE FROM item_tags WHERE subject_type = ? AND subject_id = ?", arguments: [subject.0, subject.1])
                try db.execute(sql: "DELETE FROM jobs WHERE subject_type = ? AND subject_id = ?", arguments: [subject.0, subject.1])
            }
            return deleted
        }
    }
    public func observeAll() -> ValueObservation<ValueReducers.Fetch<[Record]>> {
        ValueObservation.tracking { try Record.fetchAll($0) }
    }
}

extension Repository where Record == Meeting {
    public func observeNewestFirst() -> ValueObservation<ValueReducers.Fetch<[Meeting]>> {
        ValueObservation.tracking { try Meeting.order(Column("started_at").desc).fetchAll($0) }
    }
    public func newestFirst() throws -> [Meeting] {
        try pool.read { try Meeting.order(Column("started_at").desc).fetchAll($0) }
    }
}
extension Repository where Record == Segment {
    public func forTranscript(_ id: String) throws -> [Segment] {
        try pool.read { try Segment.filter(Column("transcript_id") == id).order(Column("start_ms")).fetchAll($0) }
    }
}
extension Repository where Record == Job {
    public func observeState(_ state: String) -> ValueObservation<ValueReducers.Fetch<[Job]>> {
        ValueObservation.tracking { try Job.filter(Column("state") == state).order(Column("created_at")).fetchAll($0) }
    }
    public func byState(_ state: String) throws -> [Job] {
        try pool.read { try Job.filter(Column("state") == state).order(Column("created_at")).fetchAll($0) }
    }
}
