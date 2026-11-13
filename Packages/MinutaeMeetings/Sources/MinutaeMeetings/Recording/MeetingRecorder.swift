import Foundation
import MinutaeAudio
import MinutaeCore

/// The capture is accessed only by its owning MeetingRecorder actor.
private final class CaptureBox: @unchecked Sendable {
    let capture: any AudioCapture
    init(_ capture: any AudioCapture) { self.capture = capture }
}

public enum MeetingRecorderError: Error, Sendable {
    case alreadyRecording
    case notRecording
    case finishFailed(meetingId: String, reason: String)
}

public actor MeetingRecorder {
    private let database: MinutaeDatabase
    private let directory: URL
    private let capture: CaptureBox
    private var id: String?
    private var writer: WAVWriter?
    private var consumer: Task<Void, Never>?
    private var paused = false
    private var writeError: String?
    private var lastStopped: String?

    public init(database: MinutaeDatabase, recordingsDirectory: URL, capture: any AudioCapture) {
        self.database = database
        self.directory = recordingsDirectory
        self.capture = CaptureBox(capture)
    }

    @discardableResult public func start(title: String = "Meeting") throws -> String {
        guard id == nil else { throw MeetingRecorderError.alreadyRecording }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let meetingId = UUID().uuidString.lowercased()
        let now = timestamp()
        let meeting = Meeting(id: meetingId, title: title, started_at: now, ended_at: nil, duration_seconds: nil, status: "recording")
        let newWriter = try WAVWriter(url: path(meetingId))
        do {
            try database.pool.write { db in try meeting.insert(db) }
            let stream = capture.capture.subscribe()
            try capture.capture.start()
            id = meetingId; writer = newWriter; paused = false; writeError = nil; lastStopped = nil
            consumer = Task { [weak self] in
                for await samples in stream { await self?.append(samples) }
            }
            return meetingId
        } catch {
            try? newWriter.close()
            throw error
        }
    }

    public func pause() throws {
        guard id != nil else { throw MeetingRecorderError.notRecording }
        paused = true
    }
    public func resume() throws {
        guard id != nil else { throw MeetingRecorderError.notRecording }
        paused = false
    }
    private func append(_ samples: [Float]) {
        guard !paused, writeError == nil else { return }
        do { try writer?.append(samples) } catch { writeError = String(describing: error) }
    }

    @discardableResult public func stop() async throws -> String {
        guard let meetingId = id else {
            if let lastStopped { return lastStopped }
            throw MeetingRecorderError.notRecording
        }
        do {
            _ = try capture.capture.stop()
            await consumer?.value
            try writer?.close()
            writer = nil; consumer = nil; id = nil
            if let writeError { throw MeetingRecorderError.finishFailed(meetingId: meetingId, reason: writeError) }
            try finishStop(meetingId: meetingId)
            lastStopped = meetingId
            return meetingId
        } catch let error as MeetingRecorderError { throw error }
        catch { throw MeetingRecorderError.finishFailed(meetingId: meetingId, reason: String(describing: error)) }
    }

    public func finishStop(meetingId: String) throws {
        do {
            let url = path(meetingId)
            let size = try fileSize(url)
            let now = timestamp()
            let duration = max(0, size - 44) / 64_000
            let recording = Recording(id: UUID().uuidString.lowercased(), meeting_id: meetingId, file_path: url.path, file_size_bytes: size, created_at: now)
            let job = Job(id: UUID().uuidString.lowercased(), kind: "transcribe", subject_type: "meeting", subject_id: meetingId, payload: nil, state: "queued", attempts: 0, max_attempts: 2, last_error: nil, run_after: nil, created_at: now, updated_at: now)
            try database.pool.write { db in
                guard var meeting = try Meeting.fetchOne(db, key: meetingId) else { throw MeetingRecorderError.notRecording }
                guard meeting.status == "recording" else { return }
                meeting.ended_at = now; meeting.duration_seconds = duration; meeting.status = "processing"
                try meeting.update(db); try recording.insert(db); try job.insert(db)
            }
            lastStopped = meetingId
        } catch { throw MeetingRecorderError.finishFailed(meetingId: meetingId, reason: String(describing: error)) }
    }

    public func recover() throws {
        let meetings = try Repository<Meeting>(database).all().filter { $0.status == "recording" }
        for meeting in meetings {
            let url = path(meeting.id)
            if let size = try? fileSize(url), size > 44 {
                try repairHeader(url, size: size)
                try finishStop(meetingId: meeting.id)
            } else {
                try database.pool.write { db in
                    guard var row = try Meeting.fetchOne(db, key: meeting.id), row.status == "recording" else { return }
                    row.status = "failed"; try row.update(db)
                }
            }
        }
    }
    private func repairHeader(_ url: URL, size: Int) throws {
        let bytes = (size - 44) / 4 * 4
        guard bytes <= Int(UInt32.max - 36) else { throw CocoaError(.fileReadCorruptFile) }
        let file = try FileHandle(forUpdating: url)
        defer { try? file.close() }
        var riff = UInt32(bytes + 36).littleEndian
        var data = UInt32(bytes).littleEndian
        try file.seek(toOffset: 4); try file.write(contentsOf: Data(bytes: &riff, count: 4))
        try file.seek(toOffset: 40); try file.write(contentsOf: Data(bytes: &data, count: 4))
        try file.synchronize()
    }
    private func path(_ id: String) -> URL { directory.appendingPathComponent("\(id).wav") }
    private func fileSize(_ url: URL) throws -> Int {
        let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
        return (attributes[.size] as? NSNumber)?.intValue ?? 0
    }
    private func timestamp() -> String { ISO8601DateFormatter().string(from: Date()) }
}
