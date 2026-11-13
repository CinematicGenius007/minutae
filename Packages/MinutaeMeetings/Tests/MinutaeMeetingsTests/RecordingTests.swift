import Foundation
import Testing
import MinutaeAudio
import MinutaeCore
@testable import MinutaeMeetings

@Test func recordingStopAndRetry() async throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: dir) }
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let db = try MinutaeDatabase(path: dir.appendingPathComponent("test.db").path)
    let recorder = MeetingRecorder(database: db, recordingsDirectory: dir, capture: FakeAudioCapture(samples: Array(repeating: 0.5, count: 16_000)))
    let id = try await recorder.start()
    #expect(try await recorder.stop() == id)
    #expect(try await recorder.stop() == id)
    try await recorder.finishStop(meetingId: id)
    #expect(try Repository<Meeting>(db).find(id)?.status == "processing")
    #expect(try Repository<Recording>(db).all().count == 1)
    #expect(try Repository<Job>(db).all().count == 1)
}

@Test func recoveryAndFailedTransaction() async throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: dir) }
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let db = try MinutaeDatabase(path: dir.appendingPathComponent("test.db").path)
    let recorder = MeetingRecorder(database: db, recordingsDirectory: dir, capture: FakeAudioCapture(samples: Array(repeating: 0.5, count: 16_000)))
    let id = try await recorder.start()
    try await db.pool.write { db in try db.execute(sql: "CREATE TRIGGER reject_recording BEFORE INSERT ON recordings BEGIN SELECT RAISE(ABORT, 'rejected'); END") }
    do {
        _ = try await recorder.stop()
        Issue.record("stop should fail")
    } catch MeetingRecorderError.finishFailed(let failedId, _) {
        #expect(failedId == id)
    }
    #expect(FileManager.default.fileExists(atPath: dir.appendingPathComponent("\(id).wav").path))
    #expect(try Repository<Meeting>(db).find(id)?.status == "recording")
    #expect(try Repository<Job>(db).all().isEmpty)
    try await db.pool.write { db in try db.execute(sql: "DROP TRIGGER reject_recording") }
    try await recorder.recover()
    #expect(try Repository<Meeting>(db).find(id)?.status == "processing")
    #expect(try Repository<Recording>(db).all().count == 1)
    #expect(try Repository<Job>(db).all().count == 1)
}
