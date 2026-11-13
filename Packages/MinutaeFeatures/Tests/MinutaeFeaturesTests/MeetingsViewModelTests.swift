import Foundation
import MinutaeAudio
import MinutaeCore
import MinutaeMeetings
import Testing

@testable import MinutaeFeatures

@MainActor @Test func listAndDetailPersistence() async throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: dir) }
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let db = try MinutaeDatabase(path: dir.appendingPathComponent("test.db").path)
    let runner = JobRunner(database: db, handlers: [:])
    let meeting = Meeting(
        id: "m", title: "Design sync", started_at: "2026-01-01T10:00:00Z", ended_at: nil,
        duration_seconds: 42, status: "done")
    try Repository<Meeting>(db).save(meeting)
    let item = ActionItem(
        id: "a", meeting_id: "m", text: "Follow up", done: false, created_at: "2026-01-01T10:00:00Z"
    )
    try Repository<ActionItem>(db).save(item)
    let list = MeetingsViewModel(database: db, runner: runner, summariesDirectory: dir)
    list.refresh()
    list.search = "DESIGN"
    #expect(list.filtered.count == 1)
    list.search = "absent"
    #expect(list.filtered.isEmpty)
    let detail = MeetingDetailViewModel(
        meeting: meeting, database: db, runner: runner, summariesDirectory: dir)
    detail.rename("New title")
    detail.toggle(item)
    #expect(try Repository<Meeting>(db).find("m")?.title == "New title")
    #expect(try Repository<ActionItem>(db).find("a")?.done == true)
    await detail.rerun()
    #expect(try Repository<Job>(db).all().first?.kind == "summarise")
    detail.delete()
    #expect(detail.deleted)
    #expect(try Repository<Meeting>(db).find("m") == nil)
    #expect(try Repository<Job>(db).all().isEmpty)
}

@MainActor @Test func captureAdapterStartsAndStops() async throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: dir) }
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let db = try MinutaeDatabase(path: dir.appendingPathComponent("test.db").path)
    let recorder = MeetingRecorder(
        database: db, recordingsDirectory: dir, capture: FakeAudioCapture(samples: [0.1]))
    let model = MeetingCaptureViewModel(recorder: recorder)
    await model.start()
    #expect(model.startedAt != nil)
    await model.stop()
    #expect(model.startedAt == nil)
    #expect(model.errorMessage == nil)
    #expect(try Repository<Meeting>(db).all().first?.status == "processing")
}
