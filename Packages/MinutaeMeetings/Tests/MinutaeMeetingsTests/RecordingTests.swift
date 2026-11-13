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

@Test func pauseResumeAndStopPaused() async throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: dir) }
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let db = try MinutaeDatabase(path: dir.appendingPathComponent("test.db").path)
    let fake = FakeAudioCapture(samples: [0.1])
    let recorder = MeetingRecorder(database: db, recordingsDirectory: dir, capture: fake)
    let id = try await recorder.start()
    try await Task.sleep(for: .milliseconds(50))
    try await recorder.pause()
    try await recorder.pause()
    try fake.buffer.push([0.2, 0.2])
    try await Task.sleep(for: .milliseconds(50))
    try await recorder.resume()
    try await recorder.resume()
    try fake.buffer.push([0.3, 0.3, 0.3])
    try await Task.sleep(for: .milliseconds(50))
    try await recorder.pause()
    try fake.buffer.push([0.4, 0.4])
    try await Task.sleep(for: .milliseconds(50))
    _ = try await recorder.stop()
    let data = try Data(contentsOf: dir.appendingPathComponent("\(id).wav"))
    #expect(data.count == 44 + 4 * 4)
    let samples = stride(from: 44, to: data.count, by: 4).map { offset in
        Float(bitPattern: data[offset..<(offset + 4)].enumerated().reduce(UInt32(0)) {
            $0 | (UInt32($1.element) << (8 * $1.offset))
        })
    }
    #expect(samples == [0.1, 0.3, 0.3, 0.3])
}
