import Foundation
import Testing
import MinutaeAudio
import MinutaeCore
import MinutaeSpeech
@testable import MinutaeMeetings

private final class ProgressValues: @unchecked Sendable {
    private let lock = NSLock()
    private var storage: [Double] = []
    func add(_ value: Double) { lock.lock(); defer { lock.unlock() }; storage.append(value) }
    var values: [Double] { lock.lock(); defer { lock.unlock() }; return storage }
}

@Test func transcriptionResumesAndEnqueuesSummary() async throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let db = try MinutaeDatabase(path: dir.appendingPathComponent("test.db").path)
    let recorder = MeetingRecorder(database: db, recordingsDirectory: dir, capture: FakeAudioCapture(samples: Array(repeating: 0.5, count: 32_000)))
    let meeting = try await recorder.start()
    _ = try await recorder.stop()
    let job = try #require(Repository<Job>(db).all().first)
    let fake = FakeSpeechEngine(result: MinutaeSpeech.Transcript(segments: [.init(text: "second", startMilliseconds: 0, endMilliseconds: 1000)], language: "en"))
    await fake.load(model: dir)
    let now = ISO8601DateFormatter().string(from: Date())
    try await db.pool.write { db in
        try MinutaeCore.Transcript(id: job.id, meeting_id: meeting, raw_text: "", provider: "whisper-local", language: nil, duration_ms: nil, created_at: now).insert(db)
        try Segment(id: UUID().uuidString, transcript_id: job.id, start_ms: 0, end_ms: 1000, text: "first", speaker_label: nil).insert(db)
    }
    let progress = ProgressValues()
    let handler = TranscribeJobHandler(database: db, engine: fake, progress: { progress.add($0) })
    try await handler.run(job)
    try await handler.run(job)
    let segments = try Repository<Segment>(db).all().sorted { $0.start_ms < $1.start_ms }
    #expect(segments.map(\.text) == ["first", "second"])
    #expect(segments.map(\.start_ms) == [0, 1000])
    #expect(try Repository<Job>(db).all().filter { $0.kind == "summarise" }.count == 1)
    #expect(progress.values == progress.values.sorted())
    #expect(progress.values.last == 1)
}

@Test func truncatedRecordingAndEngineFailure() async throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let db = try MinutaeDatabase(path: dir.appendingPathComponent("test.db").path)
    let recorder = MeetingRecorder(database: db, recordingsDirectory: dir, capture: FakeAudioCapture(samples: Array(repeating: 0.5, count: 16_000)))
    let meeting = try await recorder.start()
    _ = try await recorder.stop()
    let job = try #require(Repository<Job>(db).all().first)
    let path = dir.appendingPathComponent("\(meeting).wav")
    let file = try FileHandle(forUpdating: path)
    try file.truncate(atOffset: 44 + 8000 * 4 + 2)
    try file.close()
    let fake = FakeSpeechEngine(result: .init(segments: [.init(text: "partial", startMilliseconds: 0, endMilliseconds: 500)], language: nil))
    let handler = TranscribeJobHandler(database: db, engine: fake)
    do {
        try await handler.run(job)
        Issue.record("Unloaded engine must fail")
    } catch SpeechError.notLoaded { }
    #expect(try Repository<Job>(db).all().filter { $0.kind == "summarise" }.isEmpty)
    await fake.load(model: dir)
    try await handler.run(job)
    #expect(try Repository<Segment>(db).all().map(\.text) == ["partial"])
    #expect(try Repository<MinutaeCore.Transcript>(db).find(job.id)?.duration_ms == 500)
}
