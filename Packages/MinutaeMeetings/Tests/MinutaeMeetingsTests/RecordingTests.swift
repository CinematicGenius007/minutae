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
