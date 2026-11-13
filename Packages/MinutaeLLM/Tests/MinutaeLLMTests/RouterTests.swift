import Foundation
import Testing
import MinutaeCore
@testable import MinutaeLLM

private actor Fake: LLMTransport {
    nonisolated let id: TransportID
    var state: TransportStatus
    var results: [Result<String, LLMError>]
    var requests: [LLMRequest] = []
    var statusCount = 0
    var statusDelay: Duration?
    var runDelay: Duration?
    init(_ id: TransportID, status: TransportStatus = .init(installed: true), results: [Result<String, LLMError>] = [.success("ok")], statusDelay: Duration? = nil, runDelay: Duration? = nil) {
        self.id = id; self.state = status; self.results = results; self.statusDelay = statusDelay; self.runDelay = runDelay
    }
    func status() async -> TransportStatus {
        statusCount += 1
        if let statusDelay { try? await Task.sleep(for: statusDelay) }
        return state
    }
    func run(_ request: LLMRequest) async throws -> LLMResponse {
        requests.append(request)
        if let runDelay { try await Task.sleep(for: runDelay) }
        let result = results.count > 1 ? results.removeFirst() : results[0]
        return LLMResponse(text: try result.get(), transport: id, duration: .zero)
    }
    func count() -> Int { requests.count }
    func statuses() -> Int { statusCount }
    func prompts() -> [String] { requests.map(\.prompt) }
}
private actor Observer: UsageObserver {
    struct Event: Sendable { let transport: TransportID; let purpose: Purpose; let error: String? }
    var events: [Event] = []
    func record(transport: TransportID, purpose: Purpose, duration: Duration, errorClass: String?) async {
        events.append(Event(transport: transport, purpose: purpose, error: errorClass))
    }
    func all() -> [Event] { events }
}
private let request = LLMRequest(prompt: "secret prompt", timeout: .seconds(5), purpose: .summary)
private func router(_ a: Fake, _ b: Fake, policies: [Purpose: RouterPolicy] = [:], observer: (any UsageObserver)? = nil) -> LLMRouter {
    LLMRouter(chain: [.codexCLI, .grokCLI], transports: [a, b], policies: policies, observer: observer)
}
private func isCancelled(_ error: any Error) -> Bool {
    if case LLMError.cancelled = error { return true }; return false
}

@Test func fallbackRules() async throws {
    let errors: [LLMError] = [.notInstalled, .notLoggedIn, .rateLimited(retryAfter: nil), .timeout, .modelRejected("x")]
    for error in errors {
        let a = Fake(.codexCLI, results: [.failure(error)])
        let b = Fake(.grokCLI)
        let response = try await router(a, b).run(request)
        #expect(response.transport == .grokCLI)
        #expect(await b.count() == 1)
    }
    for status in [TransportStatus(installed: false), TransportStatus(installed: true, signedIn: false)] {
        let a = Fake(.codexCLI, status: status); let b = Fake(.grokCLI)
        _ = try await router(a, b).run(request)
        #expect(await a.count() == 0)
        #expect(await b.count() == 1)
    }
    for error in [LLMError.unavailable("x"), .invalidOutput("x"), .cancelled] {
        let a = Fake(.codexCLI, results: [.failure(error)]); let b = Fake(.grokCLI)
        do { _ = try await router(a, b).run(request); Issue.record("Expected failure") } catch {}
        #expect(await b.count() == 0)
    }
}

@Test func cancellationAtEachStage() async throws {
    for stage in 0..<3 {
        let a = Fake(.codexCLI, statusDelay: stage == 1 ? .seconds(30) : nil, runDelay: stage == 2 ? .seconds(30) : nil)
        let b = Fake(.grokCLI)
        let task = Task {
            if stage == 0 { withUnsafeCurrentTask { $0?.cancel() } }
            return try await router(a, b).run(request)
        }
        if stage != 0 {
            if stage == 2 { while await a.count() == 0 { await Task.yield() } }
            else { while await a.statuses() == 0 { await Task.yield() } }
            task.cancel()
        }
        do { _ = try await task.value; Issue.record("Expected cancellation") }
        catch { #expect(isCancelled(error)) }
        #expect(await b.count() == 0)
        if stage < 2 { #expect(await a.count() == 0) }
        if stage == 0 { #expect(await a.statuses() == 0) }
    }
}

@Test func aggregateAndPolicyAndObserver() async throws {
    let a = Fake(.codexCLI, results: [.failure(.rateLimited(retryAfter: nil))]); let b = Fake(.grokCLI, results: [.failure(.timeout)])
    let observer = Observer()
    do { _ = try await router(a, b, observer: observer).run(request); Issue.record("Expected aggregate") }
    catch let error as RouterAggregateError {
        #expect(error.failures.map(\.errorClass) == ["rateLimited", "timeout"])
        #expect(!String(describing: error).contains("secret prompt"))
    }
    let events = await observer.all()
    #expect(events.count == 2)
    #expect(events.map(\.error) == ["rateLimited", "timeout"])
    #expect(!String(describing: events).contains("secret prompt"))
    let c = Fake(.codexCLI); let d = Fake(.grokCLI)
    let policy = RouterPolicy(chain: [.grokCLI], timeout: .seconds(2))
    _ = try await router(c, d, policies: [.summary: policy]).run(request)
    #expect(await c.count() == 0)
    #expect(await d.requests.first?.timeout == .seconds(2))
}

private struct Payload: Decodable, Sendable { let value: Int }
@Test func jsonRepair() async throws {
    let a = Fake(.codexCLI, results: [.success("bad"), .success("{\"value\":42}")]); let b = Fake(.grokCLI)
    let value = try await router(a, b).run(request, as: Payload.self)
    #expect(value.value == 42)
    #expect(await a.count() == 2)
    #expect(await b.count() == 0)
    let c = Fake(.codexCLI, results: [.success("bad"), .success("still bad")]); let d = Fake(.grokCLI, results: [.success("{\"value\":7}")])
    let fallback = try await router(c, d).run(request, as: Payload.self)
    #expect(fallback.value == 7)
    #expect(await c.count() == 2)
    #expect(await d.count() == 1)
    let e = Fake(.codexCLI, results: [.success("bad")]); let f = Fake(.grokCLI, results: [.success("bad")])
    do { _ = try await router(e, f).run(request, as: Payload.self); Issue.record("Expected aggregate") }
    catch let error as RouterAggregateError { #expect(error.failures.map(\.errorClass) == ["invalidOutput", "invalidOutput"]) }
    #expect(await e.count() == 2); #expect(await f.count() == 2)
    let g = Fake(.codexCLI, results: [.failure(.invalidOutput("transport"))]); let h = Fake(.grokCLI)
    do { _ = try await router(g, h).run(request, as: Payload.self); Issue.record("Expected failure") } catch {}
    #expect(await h.count() == 0)
}

@Test func longTranscriptReduction() async throws {
    let transcript = Array(repeating: "meeting ", count: 20_000).joined()
    let budget = 100
    let chunks = LLMRouter.chunks(transcript, tokenBudget: budget, overlapTokens: 10)
    #expect(chunks.count > 1)
    #expect(chunks.allSatisfy { $0.count <= budget * 4 })
    let a = Fake(.codexCLI, results: [.success(String(repeating: "s", count: 40))]); let b = Fake(.grokCLI)
    _ = try await router(a, b).summarizeLong(transcript: transcript, request: request, tokenBudget: budget, overlapTokens: 10)
    var count = chunks.count; var calls = chunks.count
    while count * 40 + max(0, count - 1) * 2 > budget * 4 {
        count = (count + 8) / 9; calls += count
    }
    calls += 1
    #expect(await a.count() == calls)
    let prompts = await a.prompts()
    let prefix = "Combine these partial summaries into a coherent summary:\n\n"
    #expect(prompts.filter { $0.hasPrefix(prefix) }.allSatisfy { $0.dropFirst(prefix.count).count <= budget * 4 })
}

@Test func reductionDepthGuard() async throws {
    let a = Fake(.codexCLI, results: [.success(String(repeating: "x", count: 40))])
    let b = Fake(.grokCLI)
    do {
        _ = try await router(a, b).summarizeLong(transcript: String(repeating: "x", count: 100), request: request, tokenBudget: 10, maxDepth: 2)
        Issue.record("Expected depth guard")
    } catch LLMError.invalidOutput { } catch { Issue.record("Unexpected error: \(error)") }
    #expect(await a.count() == 9)
}
