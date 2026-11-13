import Foundation
import MinutaeCore
import MinutaeLLM

/// Summarises one meeting. The summary row id is the job id, so a retry does not
/// insert a second row. A later job for the same meeting keeps its own version.
public struct SummariseJobHandler: Sendable {
    private let database: MinutaeDatabase
    private let generator: any SummaryGenerating
    private let summariesDirectory: URL
    private let clock: @Sendable () -> Date

    public init(
        database: MinutaeDatabase,
        generator: any SummaryGenerating,
        summariesDirectory: URL,
        clock: @escaping @Sendable () -> Date = { Date() }
    ) {
        self.database = database
        self.generator = generator
        self.summariesDirectory = summariesDirectory
        self.clock = clock
    }

    public var handler: JobHandler { { job in try await run(job) } }

    public func run(_ job: Job) async throws {
        do {
            try Task.checkCancellation()
            try await perform(job)
        } catch is CancellationError {
            throw CancellationError()
        } catch let error as LLMError where error.isCancellation {
            throw CancellationError()
        } catch let error as SummaryJobError {
            try await fail(job, error)
        } catch {
            try await fail(job, SummaryJobError.classify(error))
        }
    }

    private struct Source: Sendable {
        var meeting: Meeting
        var transcript: String
        var recordingFilename: String
    }

    private func perform(_ job: Job) async throws {
        guard job.kind == "summarise", job.subject_type == "meeting" else {
            throw SummaryJobError(
                errorClass: "unavailable",
                message: "This job is not a summary job."
            )
        }
        if try await summaryExists(job.id) {
            try await markDone(job.subject_id)
            return
        }
        let source = try await load(job)
        let generation = try await generator.generate(transcript: source.transcript)
        let rendered = SummaryMarkdown.render(
            meeting: source.meeting,
            summary: generation.summary,
            recordingFilename: source.recordingFilename
        )
        let destination = try SummaryMarkdown.destination(
            directory: summariesDirectory,
            startedAt: source.meeting.started_at,
            title: generation.summary.title,
            meetingID: source.meeting.id
        )
        do {
            try SummaryMarkdown.atomicWrite(rendered.file, to: destination)
        } catch is CancellationError {
            throw CancellationError()
        } catch let error as SummaryJobError {
            throw error
        } catch {
            throw SummaryJobError(
                errorClass: "unavailable",
                message: "The summary file could not be saved."
            )
        }
        do {
            try await persist(job: job, generation: generation, body: rendered.body)
        } catch is CancellationError {
            try? FileManager.default.removeItem(at: destination)
            throw CancellationError()
        } catch let error as SummaryJobError {
            try? FileManager.default.removeItem(at: destination)
            throw error
        } catch {
            try? FileManager.default.removeItem(at: destination)
            throw SummaryJobError(
                errorClass: "unavailable",
                message: "The summary could not be saved."
            )
        }
    }

    private func load(_ job: Job) async throws -> Source {
        try await database.pool.read { database in
            guard let meeting = try Meeting.fetchOne(database, key: job.subject_id) else {
                throw SummaryJobError(
                    errorClass: "unavailable",
                    message: "The meeting could not be found."
                )
            }
            let transcript = try Transcript.fetchOne(
                database,
                sql: """
                    SELECT * FROM transcripts WHERE meeting_id = ? \
                    ORDER BY created_at DESC LIMIT 1
                    """,
                arguments: [job.subject_id]
            )
            let text = transcript?.raw_text.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            guard !text.isEmpty else {
                throw SummaryJobError(
                    errorClass: "unavailable",
                    message: "This meeting has no transcript to summarise."
                )
            }
            let recording = try Recording.fetchOne(
                database,
                sql: """
                    SELECT * FROM recordings WHERE meeting_id = ? \
                    ORDER BY created_at DESC LIMIT 1
                    """,
                arguments: [job.subject_id]
            )
            return Source(
                meeting: meeting,
                transcript: text,
                recordingFilename: SummaryMarkdown.recordingFilename(
                    meetingID: meeting.id,
                    path: recording?.file_path
                )
            )
        }
    }

    private func persist(job: Job, generation: SummaryGeneration, body: String) async throws {
        let now = Self.timestamp(clock())
        try await database.pool.write { database in
            if try Summary.fetchOne(database, key: job.id) != nil { return }
            try Summary(
                id: job.id,
                meeting_id: job.subject_id,
                markdown_content: body,
                model: generation.model,
                provider: generation.provider,
                created_at: now
            ).insert(database)
            for item in generation.summary.actionItems {
                try ActionItem(
                    id: UUID().uuidString.lowercased(),
                    meeting_id: job.subject_id,
                    text: item.checklistText,
                    done: false,
                    created_at: now
                ).insert(database)
            }
            guard var meeting = try Meeting.fetchOne(database, key: job.subject_id) else {
                throw SummaryJobError(
                    errorClass: "unavailable",
                    message: "The meeting could not be found."
                )
            }
            meeting.status = "done"
            meeting.title = generation.summary.title
            try meeting.update(database)
        }
    }

    private func summaryExists(_ id: String) async throws -> Bool {
        try await database.pool.read { try Summary.fetchOne($0, key: id) != nil }
    }

    private func markDone(_ meetingID: String) async throws {
        try await database.pool.write { database in
            guard var meeting = try Meeting.fetchOne(database, key: meetingID),
                meeting.status != "done"
            else { return }
            meeting.status = "done"
            try meeting.update(database)
        }
    }

    /// A retryable attempt leaves the meeting status alone. The last attempt,
    /// the one `JobRunner` will not run again, marks the meeting failed.
    private func fail(_ job: Job, _ error: SummaryJobError) async throws -> Never {
        if job.attempts >= job.max_attempts {
            _ = try? await database.pool.write { database in
                guard var meeting = try Meeting.fetchOne(database, key: job.subject_id),
                    meeting.status != "done"
                else { return }
                meeting.status = "failed"
                try meeting.update(database)
            }
        }
        throw error
    }

    private static func timestamp(_ date: Date) -> String {
        ISO8601DateFormatter().string(from: date)
    }
}
