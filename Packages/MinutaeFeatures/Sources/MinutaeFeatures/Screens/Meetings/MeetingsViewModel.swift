import Foundation
import MinutaeCore
import Observation

@MainActor @Observable
public final class MeetingsViewModel {
    public var search = ""
    public private(set) var meetings: [Meeting] = []
    public private(set) var jobs: [Job] = []
    public private(set) var error: String?
    public let database: MinutaeDatabase
    public let runner: JobRunner
    public let summariesDirectory: URL

    public init(database: MinutaeDatabase, runner: JobRunner, summariesDirectory: URL) {
        self.database = database
        self.runner = runner
        self.summariesDirectory = summariesDirectory
    }

    public var filtered: [Meeting] {
        meetings.filter { search.isEmpty || $0.title.localizedCaseInsensitiveContains(search) }
    }

    public func refresh() {
        do {
            meetings = try Repository<Meeting>(database).newestFirst()
            jobs = try Repository<Job>(database).all()
            error = nil
        } catch { self.error = error.localizedDescription }
    }

    public func status(for meeting: Meeting) -> String {
        let relevant = jobs.filter {
            $0.subject_type == "meeting" && $0.subject_id == meeting.id
                && ["transcribe", "summarise"].contains($0.kind)
        }
        if relevant.contains(where: { $0.state == "queued" || $0.state == "running" }) {
            return "Processing"
        }
        if let latest = relevant.max(by: { $0.created_at < $1.created_at }),
            latest.state == "failed"
        {
            return "Failed"
        }
        return meeting.status.capitalized
    }
}

@MainActor @Observable
public final class MeetingDetailViewModel {
    public private(set) var meeting: Meeting
    public private(set) var summary: Summary?
    public private(set) var items: [ActionItem] = []
    public private(set) var segments: [Segment] = []
    public private(set) var transcript: Transcript?
    public private(set) var recording: Recording?
    public private(set) var error: String?
    public private(set) var deleted = false
    private let database: MinutaeDatabase
    private let runner: JobRunner
    private let summariesDirectory: URL

    public init(
        meeting: Meeting, database: MinutaeDatabase, runner: JobRunner, summariesDirectory: URL
    ) {
        self.meeting = meeting
        self.database = database
        self.runner = runner
        self.summariesDirectory = summariesDirectory
        refresh()
    }

    public func refresh() {
        do {
            if let current = try Repository<Meeting>(database).find(meeting.id) {
                meeting = current
            }
            summary = try Repository<Summary>(database).all().filter { $0.meeting_id == meeting.id }
                .max { $0.created_at < $1.created_at }
            items = try Repository<ActionItem>(database).all().filter {
                $0.meeting_id == meeting.id
            }
            transcript = try Repository<Transcript>(database).all().filter {
                $0.meeting_id == meeting.id
            }.max { $0.created_at < $1.created_at }
            segments =
                try transcript.map { try Repository<Segment>(database).forTranscript($0.id) } ?? []
            recording = try Repository<Recording>(database).all().filter {
                $0.meeting_id == meeting.id
            }.max { $0.created_at < $1.created_at }
            error = nil
        } catch { self.error = error.localizedDescription }
    }

    public func rename(_ title: String) {
        let trimmed = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        var updated = meeting
        updated.title = trimmed
        do {
            try Repository<Meeting>(database).save(updated)
            meeting = updated
        } catch { self.error = error.localizedDescription }
    }

    public func toggle(_ item: ActionItem) {
        var updated = item
        updated.done.toggle()
        do {
            try Repository<ActionItem>(database).save(updated)
            refresh()
        } catch { self.error = error.localizedDescription }
    }

    public func rerun() async {
        do {
            _ = try await runner.enqueue(
                kind: .summarise, subjectType: "meeting", subjectID: meeting.id)
        } catch { self.error = error.localizedDescription }
    }

    public var summaryFile: URL? {
        guard
            let files = try? FileManager.default.contentsOfDirectory(
                at: summariesDirectory, includingPropertiesForKeys: nil)
        else { return nil }
        return files.first { url in
            guard url.pathExtension == "md",
                let text = try? String(contentsOf: url, encoding: .utf8)
            else { return false }
            return text.split(separator: "\n").contains(Substring("id: \(meeting.id)"))
        }
    }

    public func delete() {
        do {
            let recordings = try Repository<Recording>(database).all().filter {
                $0.meeting_id == meeting.id
            }
            let file = summaryFile
            for url in recordings.map({ URL(fileURLWithPath: $0.file_path) })
                + (file.map { [$0] } ?? [])
            {
                if FileManager.default.fileExists(atPath: url.path) {
                    try FileManager.default.removeItem(at: url)
                }
            }
            try Repository<Meeting>(database).delete(meeting)
            deleted = true
        } catch { self.error = error.localizedDescription }
    }
}
