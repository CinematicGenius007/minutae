import Foundation
import MinutaeCore
import MinutaeSpeech

/// Each job owns its transcript ID. A retry starts at the last persisted segment end.
public struct TranscribeJobHandler: Sendable {
    private let database: MinutaeDatabase
    private let engine: any SpeechEngine
    private let progress: @Sendable (Double) -> Void

    public init(database: MinutaeDatabase, engine: any SpeechEngine, progress: @escaping @Sendable (Double) -> Void = { _ in }) {
        self.database = database
        self.engine = engine
        self.progress = progress
    }

    public var handler: JobHandler { { job in try await run(job) } }

    public func run(_ job: Job) async throws {
        guard job.kind == "transcribe", job.subject_type == "meeting" else {
            throw NonRetryableJobError("Invalid transcription job")
        }
        let recording = try await database.pool.read { db in
            try Recording.fetchOne(db, sql: "SELECT * FROM recordings WHERE meeting_id = ? ORDER BY created_at DESC LIMIT 1", arguments: [job.subject_id])
        }
        guard let recording else { throw NonRetryableJobError("Recording not found") }
        let file: FileHandle
        do { file = try FileHandle(forReadingFrom: URL(fileURLWithPath: recording.file_path)) }
        catch { throw NonRetryableJobError("Cannot open recording: \(error)") }
        defer { try? file.close() }
        let header = try file.read(upToCount: 44) ?? Data()
        func number(_ offset: Int, _ count: Int) -> UInt32 {
            (0..<count).reduce(0) { $0 | UInt32(header[offset + $1]) << (8 * $1) }
        }
        guard header.count == 44, String(decoding: header[0..<4], as: UTF8.self) == "RIFF",
              String(decoding: header[8..<16], as: UTF8.self) == "WAVEfmt ", number(16, 4) == 16,
              number(20, 2) == 3, number(22, 2) == 1, number(24, 4) == 16000,
              number(34, 2) == 32, String(decoding: header[36..<40], as: UTF8.self) == "data" else {
            throw NonRetryableJobError("Unsupported WAV format")
        }
        let size = try file.seekToEnd()
        let bytes = min(UInt64(number(40, 4)), size > 44 ? size - 44 : 0) / 4 * 4
        let now = ISO8601DateFormatter().string(from: Date())
        let existing = try await database.pool.write { db -> (Int, Bool) in
            if let row = try MinutaeCore.Transcript.fetchOne(db, key: job.id) {
                let segments = try Segment.fetchAll(db, sql: "SELECT * FROM segments WHERE transcript_id = ? ORDER BY end_ms DESC LIMIT 1", arguments: [job.id])
                return (segments.first?.end_ms ?? 0, row.duration_ms != nil)
            }
            try MinutaeCore.Transcript(id: job.id, meeting_id: job.subject_id, raw_text: "", provider: "whisper-local", language: nil, duration_ms: nil, created_at: now).insert(db)
            return (0, false)
        }
        if existing.1 { progress(1); return }
        let start = min(bytes, UInt64(max(0, existing.0)) * 16 * 4)
        try file.seek(toOffset: 44 + start)
        var position = start
        progress(bytes == 0 ? 0 : Double(position) / Double(bytes))
        while position < bytes {
            try Task.checkCancellation()
            let data = try file.read(upToCount: Int(min(bytes - position, 30 * 64_000))) ?? Data()
            let count = data.count / 4
            guard count > 0 else { break }
            let samples: [Float] = (0..<count).map { i in
                let j = i * 4
                let bits = UInt32(data[j]) | UInt32(data[j+1]) << 8 | UInt32(data[j+2]) << 16 | UInt32(data[j+3]) << 24
                return Float(bitPattern: bits)
            }
            let result = try await engine.transcribe(samples: samples, options: .init())
            let offset = Int(position / 64)
            try await database.pool.write { db in
                for segment in result.segments {
                    try Segment(id: UUID().uuidString.lowercased(), transcript_id: job.id, start_ms: offset + Int(segment.startMilliseconds), end_ms: offset + Int(segment.endMilliseconds), text: segment.text, speaker_label: nil).insert(db)
                }
                if let language = result.language {
                    try db.execute(sql: "UPDATE transcripts SET language = ? WHERE id = ?", arguments: [language, job.id])
                }
            }
            position += UInt64(count * 4)
            progress(Double(position) / Double(bytes))
        }
        try Task.checkCancellation()
        try await database.pool.write { db in
            let segments = try Segment.fetchAll(db, sql: "SELECT * FROM segments WHERE transcript_id = ? ORDER BY start_ms, rowid", arguments: [job.id])
            try db.execute(sql: "UPDATE transcripts SET raw_text = ?, duration_ms = ? WHERE id = ?", arguments: [segments.map(\.text).joined(separator: " "), Int(bytes / 64), job.id])
            let next = Job(id: UUID().uuidString.lowercased(), kind: "summarise", subject_type: "meeting", subject_id: job.subject_id, payload: nil, state: "queued", attempts: 0, max_attempts: 3, last_error: nil, run_after: nil, created_at: now, updated_at: now)
            try next.insert(db)
        }
        progress(1)
    }
}
