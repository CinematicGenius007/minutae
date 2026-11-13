import Foundation
import MinutaeCore
import MinutaeLLM
import Testing

@testable import MinutaeMeetings

private let legacyFixture = """
---
id: mtg_01HQZX7K3A
title: "Q2 \\"Design\\" Sync"
date: 2026-04-11T14:30:00
duration: 32m 14s
participants: [Alice, Bob, Carol]
tags: []
recording: ../recordings/2026-04-11-abc123.wav
---

## Summary
- Decided to push the redesign launch to May
- Alice owns the updated mockups

## Key Decisions
- Ship the redesign in May

## Action Items
- [ ] Alice to share updated mockups by Friday
- [ ] Bob to review the performance budget
""" + "\n"

private let fixtureSummary = MeetingSummary(
    title: "Q2 \"Design\" Sync",
    summary: [
        "Decided to push the redesign launch to May",
        "Alice owns the updated mockups"
    ],
    keyDecisions: ["Ship the redesign in May"],
    actionItems: [
        SummaryActionItem(text: "Alice to share updated mockups by Friday"),
        SummaryActionItem(text: "Bob to review the performance budget")
    ],
    participants: ["Alice", "Bob", "Carol"]
)

@Test func summaryFileMatchesLegacyFixture() async throws {
    let harness = try Fixture()
    defer { harness.remove() }
    let generation = SummaryGeneration(summary: fixtureSummary, model: "fixture-model", provider: "fixture")
    let generator = ScriptedGenerator([ { generation } ])
    let handler = harness.handler(generator)
    try await handler.run(harness.job(attempts: 1, maxAttempts: 3))
    let url = harness.directory.appendingPathComponent("2026-04-11-q2-design-sync.md")
    let file = try String(contentsOf: url, encoding: .utf8)
    #expect(file == legacyFixture)
    let summaries = try Repository<Summary>(harness.database).all()
    let stored = try #require(summaries.first)
    #expect(stored.markdown_content == legacyBody)
    #expect(file.hasSuffix(stored.markdown_content))
    #expect(stored.model == "fixture-model")
    #expect(stored.provider == "fixture")
    #expect(stored.id == "job-summary-1")
    let found = try Repository<Meeting>(harness.database).find("mtg_01HQZX7K3A")
    let meeting = try #require(found)
    #expect(meeting.status == "done")
    #expect(meeting.title == "Q2 \"Design\" Sync")
    let items = try actionItems(harness.database, "mtg_01HQZX7K3A")
    #expect(items.map(\.text) == fixtureSummary.actionItems.map(\.text))
    #expect(items.allSatisfy { $0.done == false && $0.meeting_id == "mtg_01HQZX7K3A" })
    try await handler.run(harness.job(attempts: 2, maxAttempts: 3))
    #expect(try Repository<Summary>(harness.database).all().count == 1)
    #expect(try actionItems(harness.database, "mtg_01HQZX7K3A").count == 2)
    #expect(await generator.calls == 1)
}

@Test func slugMatchesLegacyRules() {
    #expect(SummaryMarkdown.slug(for: "") == "meeting")
    #expect(SummaryMarkdown.slug(for: "!!!") == "")
    #expect(SummaryMarkdown.slug(for: "Hello, World!") == "hello-world")
    #expect(SummaryMarkdown.slug(for: "Q2 Design Sync") == "q2-design-sync")
    #expect(SummaryMarkdown.slug(for: "---hello---") == "hello")
    #expect(SummaryMarkdown.slug(for: "  Hello   World  ") == "hello-world")
    #expect(SummaryMarkdown.slug(for: "Café au lait") == "caf-au-lait")
    #expect(SummaryMarkdown.slug(for: String(repeating: "a", count: 80)) == String(repeating: "a", count: 60))
    let repeated = SummaryMarkdown.slug(for: String(repeating: "a ", count: 40))
    #expect(repeated == "a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-a-")
    #expect(repeated.count == 60)
    #expect(repeated.hasSuffix("-"))
    #expect(SummaryMarkdown.formatDurationLong(45) == "45s")
    #expect(SummaryMarkdown.formatDurationLong(60) == "1m 0s")
    #expect(SummaryMarkdown.formatDurationLong(90) == "1m 30s")
    #expect(SummaryMarkdown.formatDurationLong(3661) == "1h 1m")
    #expect(SummaryMarkdown.formatDurationLong(3600) == "1h 0m")
    #expect(SummaryMarkdown.durationText(0) == "unknown")
    #expect(SummaryMarkdown.durationText(nil) == "unknown")
}

@Test func emptySectionsAreOmitted() {
    let meeting = Meeting(
        id: "m",
        title: "Untitled",
        started_at: "2026-04-11T14:30:00",
        ended_at: nil,
        duration_seconds: 0,
        status: "processing"
    )
    let summary = MeetingSummary(
        title: "Notes",
        summary: ["Only one point"],
        keyDecisions: [],
        actionItems: [],
        participants: []
    )
    let rendered = SummaryMarkdown.render(meeting: meeting, summary: summary, recordingFilename: "m.wav")
    #expect(rendered.file.contains("duration: unknown"))
    #expect(rendered.file.contains("participants: []"))
    #expect(!rendered.file.contains("## Key Decisions"))
    #expect(!rendered.file.contains("## Action Items"))
    #expect(rendered.body == "## Summary\n- Only one point\n")
}

@Test func ownerAndDueFoldIntoTheChecklistLine() throws {
    let json = """
        {"title":"T","summary":["a"],"keyDecisions":[],"actionItems":["Send notes",\
        {"text":"File the report","owner":"Sam","due":"Monday"},\
        {"text":"","owner":"","due":""}],"participants":["Sam"]}
        """
    let summary = try #require(LLMSummaryGenerator.decode(json))
    #expect(summary.actionItems.map(\.checklistText) == ["Send notes", "Sam: File the report (due Monday)", ""])
    #expect(summary.actionItems[1].owner == "Sam")
    #expect(summary.actionItems[1].due == "Monday")
    let fenced = try #require(LLMSummaryGenerator.decode("```json\n\(json)\n```"))
    #expect(fenced.title == "T")
}

@Test func summaryPromptKeepsTheLegacyInstructions() {
    #expect(
        SummaryPrompt.system
            == """
            You are a meeting assistant. Given a meeting transcript, extract \
            structured information and return it as JSON.

            Be concise and factual. Only include information that is clearly \
            stated in the transcript.
            """
    )
    let prompt = SummaryPrompt.user(transcript: "TRANSCRIPT")
    #expect(prompt.contains("<transcript>\nTRANSCRIPT\n</transcript>"))
    #expect(prompt.contains("3-5 concise bullet points"))
    #expect(prompt.contains("Return ONLY the JSON object, no markdown fences, no explanation"))
    #expect(prompt.contains("\"owner\": \"Alice\""))
}

@Test func collisionKeepsTheOtherMeetingFile() async throws {
    let harness = try Fixture()
    defer { harness.remove() }
    let occupied = harness.directory.appendingPathComponent("2026-04-11-q2-design-sync.md")
    try FileManager.default.createDirectory(at: harness.directory, withIntermediateDirectories: true)
    try Data("id: someone-else\n".utf8).write(to: occupied)
    let handler = harness.handler(ScriptedGenerator([ { fixtureGeneration } ]))
    try await handler.run(harness.job(attempts: 1, maxAttempts: 3))
    let original = try String(contentsOf: occupied, encoding: .utf8)
    #expect(original == "id: someone-else\n")
    let url = harness.directory.appendingPathComponent("2026-04-11-q2-design-sync-2.md")
    #expect(try String(contentsOf: url, encoding: .utf8) == legacyFixture)
}

@Test func terminalFailureLeavesNoFileAndClassifiesTheError() async throws {
    let harness = try Fixture()
    defer { harness.remove() }
    let sentinel = "UNIQUE_TRANSCRIPT_SENTINEL"
    try harness.replaceTranscript(sentinel)
    let handler = harness.handler(
        ScriptedGenerator([
            { throw LLMError.invalidOutput(sentinel + " and the model output") }
        ])
    )
    do {
        try await handler.run(harness.job(attempts: 1, maxAttempts: 1))
        Issue.record("Expected the summary job to fail")
    } catch let error as SummaryJobError {
        #expect(error.errorClass == "invalidOutput")
        #expect(error.description.hasPrefix("invalidOutput: "))
        #expect(SummaryJobError.errorClass(fromLastError: String(describing: error)) == "invalidOutput")
        #expect(!error.description.contains(sentinel))
        #expect(!String(describing: error).contains("model output"))
    }
    let found = try Repository<Meeting>(harness.database).find("mtg_01HQZX7K3A")
    let meeting = try #require(found)
    #expect(meeting.status == "failed")
    #expect(try Repository<Summary>(harness.database).all().isEmpty)
    #expect(try markdownFiles(harness.directory).isEmpty)
    #expect(SummaryJobError.errorClass(fromLastError: "not a class") == nil)
}

@Test func retryAfterANonTerminalFailureWritesOneSummary() async throws {
    let harness = try Fixture()
    defer { harness.remove() }
    let generator = ScriptedGenerator([
        { throw LLMError.rateLimited(retryAfter: .seconds(9)) },
        { fixtureGeneration }
    ])
    let handler = harness.handler(generator)
    do {
        try await handler.run(harness.job(attempts: 1, maxAttempts: 3))
        Issue.record("Expected a retryable failure")
    } catch let error as SummaryJobError {
        #expect(error.errorClass == "rateLimited")
        #expect(!error.description.contains("9"))
    }
    #expect(try Repository<Meeting>(harness.database).find("mtg_01HQZX7K3A")?.status == "processing")
    #expect(try markdownFiles(harness.directory).isEmpty)
    try await handler.run(harness.job(attempts: 2, maxAttempts: 3))
    try await handler.run(harness.job(attempts: 3, maxAttempts: 3))
    #expect(try Repository<Summary>(harness.database).all().count == 1)
    #expect(try actionItems(harness.database, "mtg_01HQZX7K3A").count == 2)
    #expect(try Repository<Meeting>(harness.database).find("mtg_01HQZX7K3A")?.status == "done")
    #expect(await generator.calls == 2)
}

@Test func jobRunnerStoresTheClassifiedError() async throws {
    let harness = try Fixture()
    defer { harness.remove() }
    let handler = harness.handler(
        ScriptedGenerator([ { throw LLMError.notLoggedIn } ])
    )
    let runner = JobRunner(
        database: harness.database,
        handlers: [.summarise: handler.handler],
        delay: { _ in 0 }
    )
    let id = try await runner.enqueue(kind: .summarise, subjectType: "meeting", subjectID: "mtg_01HQZX7K3A")
    try await harness.database.pool.write { database in
        try database.execute(sql: "UPDATE jobs SET max_attempts = 1 WHERE id = ?", arguments: [id])
    }
    try await runner.start()
    var stored: Job?
    for _ in 0..<40 {
        stored = try Repository<Job>(harness.database).find(id)
        if stored?.state == "failed" { break }
        try await Task.sleep(for: .milliseconds(50))
    }
    await runner.stop()
    let job = try #require(stored)
    #expect(job.state == "failed")
    #expect(job.last_error == "notLoggedIn: Sign in to the summary provider and try again.")
    #expect(SummaryJobError.errorClass(fromLastError: job.last_error ?? "") == "notLoggedIn")
    #expect(try Repository<Meeting>(harness.database).find("mtg_01HQZX7K3A")?.status == "failed")
    #expect(try markdownFiles(harness.directory).isEmpty)
}

private let legacyBody = """
## Summary
- Decided to push the redesign launch to May
- Alice owns the updated mockups

## Key Decisions
- Ship the redesign in May

## Action Items
- [ ] Alice to share updated mockups by Friday
- [ ] Bob to review the performance budget
""" + "\n"

private let fixtureGeneration = SummaryGeneration(
    summary: fixtureSummary,
    model: "fixture-model",
    provider: "fixture"
)

private func actionItems(_ database: MinutaeDatabase, _ meetingID: String) throws -> [ActionItem] {
    try database.pool.read { database in
        try ActionItem.fetchAll(
            database,
            sql: "SELECT * FROM action_items WHERE meeting_id = ? ORDER BY rowid",
            arguments: [meetingID]
        )
    }
}

private func markdownFiles(_ directory: URL) throws -> [URL] {
    let urls = try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
    return urls.filter { $0.pathExtension == "md" || $0.lastPathComponent.contains("minutae") }
}

private struct Fixture {
    let root: URL
    let directory: URL
    let database: MinutaeDatabase

    init() throws {
        root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        directory = root.appendingPathComponent("summaries")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        database = try MinutaeDatabase(path: root.appendingPathComponent("test.db").path)
        let now = "2026-04-11T15:02:14Z"
        try database.pool.write { database in
            try Meeting(
                id: "mtg_01HQZX7K3A",
                title: "Untitled Meeting",
                started_at: "2026-04-11T14:30:00",
                ended_at: "2026-04-11T15:02:14",
                duration_seconds: 1934,
                status: "processing"
            ).insert(database)
            try Recording(
                id: "rec-1",
                meeting_id: "mtg_01HQZX7K3A",
                file_path: "/tmp/recordings/2026-04-11-abc123.wav",
                file_size_bytes: 12,
                created_at: now
            ).insert(database)
            try Transcript(
                id: "tr-1",
                meeting_id: "mtg_01HQZX7K3A",
                raw_text: "Alice and Bob discussed the redesign.",
                provider: "whisper-local",
                language: "en",
                duration_ms: 1_934_000,
                created_at: now
            ).insert(database)
        }
    }

    func remove() {
        try? FileManager.default.removeItem(at: root)
    }

    func handler(_ generator: any SummaryGenerating) -> SummariseJobHandler {
        SummariseJobHandler(database: database, generator: generator, summariesDirectory: directory)
    }

    func job(attempts: Int, maxAttempts: Int) -> Job {
        Job(
            id: "job-summary-1",
            kind: "summarise",
            subject_type: "meeting",
            subject_id: "mtg_01HQZX7K3A",
            payload: nil,
            state: "running",
            attempts: attempts,
            max_attempts: maxAttempts,
            last_error: nil,
            run_after: nil,
            created_at: "2026-04-11T15:03:00Z",
            updated_at: "2026-04-11T15:03:00Z"
        )
    }

    func replaceTranscript(_ text: String) throws {
        try database.pool.write { database in
            try database.execute(
                sql: "UPDATE transcripts SET raw_text = ? WHERE id = 'tr-1'",
                arguments: [text]
            )
        }
    }
}

private actor ScriptedGenerator: SummaryGenerating {
    private var steps: [@Sendable () throws -> SummaryGeneration]
    private(set) var calls = 0

    init(_ steps: [@Sendable () throws -> SummaryGeneration]) {
        self.steps = steps
    }

    func generate(transcript: String) async throws -> SummaryGeneration {
        let index = min(calls, steps.count - 1)
        calls += 1
        return try steps[index]()
    }
}

