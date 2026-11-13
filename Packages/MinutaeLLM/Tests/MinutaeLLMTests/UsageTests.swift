import Foundation
import MinutaeCore
import Testing

@testable import MinutaeLLM

private struct ScriptedTransport: LLMTransport {
    let id: TransportID
    let result: Result<LLMResponse, LLMError>
    func status() async -> TransportStatus {
        TransportStatus(installed: true, signedIn: true)
    }
    func run(_ request: LLMRequest) async throws -> LLMResponse {
        try result.get()
    }
}

@Test func usageAggregatesMatchTheSeed() async throws {
    let now = try timestamp("2026-11-13T15:00:00Z")
    let zone = try utc()
    try await withDatabase { database, _ in
        let observer = DatabaseUsageObserver(database: database)
        for event in try sampleEvents() {
            try await observer.record(event)
        }
        let queries = UsageQueries(database)
        try expectToday(queries.summary(.today, now: now, timeZone: zone))
        try expectWeek(queries.summary(.last7Days, now: now, timeZone: zone))
        try expectMonth(queries.summary(.last30Days, now: now, timeZone: zone))
        try expectCustomRanges(queries, now: now, timeZone: zone)
    }
}

@Test func usagePresetBoundsIncludeStartAndExcludeEnd() async throws {
    let now = try timestamp("2026-11-13T15:00:00Z")
    let zone = try utc()
    let today = UsageWindow.today.interval(now: now, timeZone: zone)
    let todayStart = try timestamp("2026-11-13T00:00:00Z")
    let todayEnd = try timestamp("2026-11-14T00:00:00Z")
    #expect(today.start == todayStart)
    #expect(today.end == todayEnd)
    let week = UsageWindow.last7Days.interval(now: now, timeZone: zone)
    let weekStart = try timestamp("2026-11-07T00:00:00Z")
    #expect(week.start == weekStart)
    let month = UsageWindow.last30Days.interval(now: now, timeZone: zone)
    let monthStart = try timestamp("2026-10-15T00:00:00Z")
    #expect(month.start == monthStart)
    try await withDatabase { database, _ in
        let observer = DatabaseUsageObserver(database: database)
        try await observer.record(
            try attempt(.codexCLI, "2026-11-13T00:00:00Z", input: 4, output: 4, milliseconds: 10))
        try await observer.record(
            try attempt(.codexCLI, "2026-11-14T00:00:00Z", input: 9, output: 9, milliseconds: 10))
        try await observer.record(
            try attempt(.ollama, "2026-10-15T00:00:00Z", input: 1, output: 1, milliseconds: 10))
        try await observer.record(
            try attempt(.ollama, "2026-10-14T23:59:59Z", input: 6, output: 6, milliseconds: 10))
        let queries = UsageQueries(database)
        let report = try queries.summary(.today, now: now, timeZone: zone)
        #expect(report.inputTokens == 4)
        #expect(report.outputTokens == 4)
        let full = try queries.summary(.last30Days, now: now, timeZone: zone)
        #expect(full.inputTokens == 5)
        #expect(full.transports.map(\.transport) == ["codexCLI", "ollama"])
    }
}

@Test func cliCostStaysNullAndUnknownTokensStayNull() async throws {
    let recordedAt = try timestamp("2026-11-13T12:00:00Z")
    try await withDatabase { database, _ in
        let observer = DatabaseUsageObserver(database: database, clock: { recordedAt })
        try await observer.record(
            UsageEvent(
                transport: .codexCLI,
                purpose: .summary,
                model: "gpt-5.4",
                usage: Usage(inputTokens: 0, outputTokens: 0, costUSD: 9),
                duration: .milliseconds(1)
            )
        )
        try await observer.record(
            UsageEvent(
                transport: .grokCLI,
                purpose: .ask,
                model: "grok-4",
                duration: .milliseconds(2)
            ))
        try await observer.record(
            UsageEvent(
                transport: .api,
                purpose: .summary,
                model: "claude",
                usage: Usage(inputTokens: 3, outputTokens: 4, costUSD: 0.25),
                duration: .milliseconds(3)
            )
        )
        await observer.record(
            transport: .ollama,
            purpose: .test,
            duration: .milliseconds(12),
            errorClass: "timeout"
        )
        try expectCostRows(Repository<APIUsageLog>(database).all())
    }
}

private func expectCostRows(_ rows: [APIUsageLog]) throws {
    #expect(rows.count == 4)
    let codex = try #require(rows.first { $0.provider == "codexCLI" })
    #expect(codex.cost_usd == nil)
    #expect(codex.input_tokens == 0)
    #expect(codex.output_tokens == 0)
    #expect(codex.endpoint == "summary")
    #expect(codex.transport == "codexCLI")
    let grok = try #require(rows.first { $0.provider == "grokCLI" })
    #expect(grok.cost_usd == nil)
    #expect(grok.input_tokens == nil)
    #expect(grok.output_tokens == nil)
    let api = try #require(rows.first { $0.provider == "api" })
    #expect(api.cost_usd == 0.25)
    #expect(api.input_tokens == 3)
    #expect(api.output_tokens == 4)
    let ollama = try #require(rows.first { $0.provider == "ollama" })
    #expect(ollama.model == "")
    #expect(ollama.endpoint == "test")
    #expect(ollama.duration_ms == 12)
    #expect(ollama.error_class == "timeout")
    #expect(ollama.cost_usd == nil)
    #expect(ollama.created_at == "2026-11-13T12:00:00Z")
    #expect(rows.allSatisfy { $0.meeting_id == nil })
}

@Test func promptTextNeverAppearsInTheUsageLog() async throws {
    let prompt = "super-secret-prompt-xyzzy-llm09"
    let output = "super-secret-output-xyzzy-llm09"
    try await withDatabase { database, url in
        let observer = DatabaseUsageObserver(database: database)
        let failed = ScriptedTransport(
            id: .codexCLI, result: .failure(.rateLimited(retryAfter: nil)))
        let worked = ScriptedTransport(
            id: .grokCLI,
            result: .success(
                LLMResponse(
                    text: output,
                    transport: .grokCLI,
                    model: "grok-4",
                    usage: Usage(inputTokens: 4, outputTokens: 2, costUSD: 8),
                    duration: .milliseconds(10)
                )
            )
        )
        let router = LLMRouter(
            chain: [.codexCLI, .grokCLI], transports: [failed, worked], observer: observer)
        let request = LLMRequest(
            system: prompt,
            prompt: prompt,
            jsonSchema: Data(prompt.utf8),
            timeout: .seconds(5),
            purpose: .summary
        )
        let response = try await router.run(request)
        #expect(response.text == output)
        let rows = try Repository<APIUsageLog>(database).all().sorted { $0.provider < $1.provider }
        #expect(rows.map(\.provider) == ["codexCLI", "grokCLI"])
        #expect(rows.map(\.error_class) == ["rateLimited", nil])
        #expect(rows.map(\.cost_usd) == [nil, nil])
        #expect(rows.map(\.model) == ["", "grok-4"])
        let grok = try #require(rows.first { $0.provider == "grokCLI" })
        #expect(grok.input_tokens == 4)
        #expect(grok.output_tokens == 2)
        let rendered = rows.map(render).joined(separator: "\n")
        #expect(!rendered.contains(prompt))
        #expect(!rendered.contains(output))
        let files = fileText(url)
        #expect(files.contains("grok-4"))
        #expect(files.contains("rateLimited"))
        #expect(!files.contains(prompt))
        #expect(!files.contains(output))
    }
}

private func expectToday(_ report: UsageReport) throws {
    #expect(report.transports.map(\.transport) == ["api", "codexCLI"])
    #expect(report.inputTokens == 110)
    #expect(report.outputTokens == 25)
    let api = try transport(report, "api")
    #expect(api.callCount == 2)
    #expect(api.successCount == 1)
    #expect(api.successRate == 0.5)
    #expect(api.averageDurationMs == 500)
    #expect(api.inputTokens == 100)
    #expect(api.outputTokens == 20)
    #expect(api.costUSD == 0.25)
    #expect(api.errors == [UsageErrorCount(errorClass: "unavailable", count: 1)])
    let codex = try transport(report, "codexCLI")
    #expect(codex.callCount == 2)
    #expect(codex.successRate == 0.5)
    #expect(codex.averageDurationMs == 200)
    #expect(codex.inputTokens == 10)
    #expect(codex.outputTokens == 5)
    #expect(codex.costUSD == nil)
    #expect(codex.errors == [UsageErrorCount(errorClass: "rateLimited", count: 1)])
}

private func expectWeek(_ report: UsageReport) throws {
    #expect(report.transports.map(\.transport) == ["api", "codexCLI"])
    #expect(report.inputTokens == 111)
    #expect(report.outputTokens == 26)
    let codex = try transport(report, "codexCLI")
    #expect(codex.callCount == 4)
    #expect(codex.successCount == 2)
    #expect(codex.successRate == 0.5)
    #expect(codex.averageDurationMs == 250)
    #expect(codex.errors.map(\.errorClass) == ["rateLimited", "timeout"])
}

private func expectMonth(_ report: UsageReport) throws {
    #expect(report.transports.map(\.transport) == ["api", "codexCLI", "grokCLI"])
    #expect(report.inputTokens == 119)
    #expect(report.outputTokens == 28)
    let grok = try transport(report, "grokCLI")
    #expect(grok.callCount == 2)
    #expect(grok.successRate == 0.5)
    #expect(grok.averageDurationMs == 100)
    #expect(grok.costUSD == nil)
    #expect(grok.inputTokens == 8)
}

private func expectCustomRanges(_ queries: UsageQueries, now: Date, timeZone: TimeZone) throws {
    let day = try timestamp("2026-11-03T00:00:00Z")
    let next = try timestamp("2026-11-04T00:00:00Z")
    let onlyGrok = try queries.summary(.range(start: day, end: next), now: now, timeZone: timeZone)
    #expect(onlyGrok.transports.map(\.transport) == ["grokCLI"])
    #expect(onlyGrok.inputTokens == 8)
    #expect(onlyGrok.outputTokens == 2)
    let oldStart = try timestamp("2026-10-01T00:00:00Z")
    let oldEnd = try timestamp("2026-10-02T00:00:00Z")
    let local = try queries.summary(
        .range(start: oldStart, end: oldEnd), now: now, timeZone: timeZone)
    let ollama = try transport(local, "ollama")
    #expect(ollama.callCount == 1)
    #expect(ollama.successRate == 1.0)
    #expect(ollama.averageDurationMs == 5_000)
    #expect(ollama.inputTokens == 7)
    #expect(ollama.outputTokens == 7)
    #expect(ollama.costUSD == nil)
    #expect(ollama.errors.isEmpty)
    #expect(local.transports.map(\.transport) == ["ollama"])
}

private func sampleEvents() throws -> [UsageEvent] {
    [
        try attempt(
            .codexCLI, "2026-11-13T10:00:00Z", model: "gpt-5.4", input: 10, output: 5, cost: 9.99,
            milliseconds: 100),
        try attempt(
            .codexCLI, "2026-11-13T11:00:00Z", model: "gpt-5.4", milliseconds: 300,
            error: "rateLimited"),
        try attempt(
            .codexCLI, "2026-11-10T08:00:00Z", model: "gpt-5.4", input: 1, output: 1,
            milliseconds: 200, purpose: .title),
        try attempt(
            .codexCLI, "2026-11-10T09:00:00Z", model: "gpt-5.4", milliseconds: 400,
            error: "timeout", purpose: .title),
        try attempt(
            .grokCLI, "2026-11-03T08:00:00Z", model: "grok-4", input: 8, output: 2, cost: 1,
            milliseconds: 50, purpose: .ask),
        try attempt(
            .grokCLI, "2026-11-03T09:00:00Z", model: "grok-4", milliseconds: 150, error: "timeout",
            purpose: .ask),
        try attempt(
            .api, "2026-11-13T12:00:00Z", model: "claude", input: 100, output: 20, cost: 0.25,
            milliseconds: 1_000),
        try attempt(
            .api, "2026-11-13T12:30:00Z", model: "claude", milliseconds: 0, error: "unavailable",
            purpose: .test),
        try attempt(
            .ollama, "2026-10-01T00:00:00Z", model: "llama", input: 7, output: 7, cost: 3,
            milliseconds: 5_000, purpose: .cleanup)
    ]
}

private func attempt(
    _ transport: TransportID,
    _ recordedAt: String,
    model: String = "m",
    input: Int? = nil,
    output: Int? = nil,
    cost: Double? = nil,
    milliseconds: Int,
    error: String? = nil,
    purpose: Purpose = .summary
) throws -> UsageEvent {
    let reported = input != nil || output != nil || cost != nil
    let usage = reported ? Usage(inputTokens: input, outputTokens: output, costUSD: cost) : nil
    return UsageEvent(
        transport: transport,
        purpose: purpose,
        model: model,
        usage: usage,
        duration: .milliseconds(milliseconds),
        errorClass: error,
        recordedAt: try timestamp(recordedAt)
    )
}

private func transport(_ report: UsageReport, _ name: String) throws -> TransportUsage {
    try #require(report.transports.first { $0.transport == name })
}

private func render(_ row: APIUsageLog) -> String {
    let inputTokens: String = row.input_tokens.map { String($0) } ?? ""
    let outputTokens: String = row.output_tokens.map { String($0) } ?? ""
    let cost: String = row.cost_usd.map { String($0) } ?? ""
    let duration: String = row.duration_ms.map { String($0) } ?? ""
    var fields: [String] = [row.id, row.meeting_id ?? "", row.provider, row.model, row.endpoint]
    fields.append(inputTokens)
    fields.append(outputTokens)
    fields.append(cost)
    fields.append(row.created_at)
    fields.append(row.transport ?? "")
    fields.append(duration)
    fields.append(row.error_class ?? "")
    return fields.joined(separator: "\t")
}

private func withDatabase(_ body: (MinutaeDatabase, URL) async throws -> Void) async throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    let url = directory.appendingPathComponent("usage.sqlite")
    let database = try MinutaeDatabase(path: url.path)
    try await body(database, url)
}

private func fileText(_ url: URL) -> String {
    ["", "-wal", "-shm"].compactMap { suffix -> String? in
        guard let data = FileManager.default.contents(atPath: url.path + suffix) else { return nil }
        // SQLite files are binary. Lossy UTF-8 keeps the text that was stored in a row.
        // swiftlint:disable:next optional_data_string_conversion
        return String(decoding: data, as: UTF8.self)
    }.joined()
}

private func timestamp(_ value: String) throws -> Date {
    try #require(ISO8601DateFormatter().date(from: value))
}

private func utc() throws -> TimeZone {
    try #require(TimeZone(secondsFromGMT: 0))
}
