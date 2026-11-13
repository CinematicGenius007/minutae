import Foundation
import MinutaeCore
import MinutaeLLM
import Testing

@testable import MinutaeMeetings

@Test func generatorUsesJsonModeAndMapReduce() async throws {
    let json = """
        {"title":"Standup","summary":["Shipped the fix"],"keyDecisions":[],\
        "actionItems":[],"participants":["Ada"]}
        """
    let transport = FakeTransport(.codexCLI, results: [.success(json)], model: "fake-model")
    let router = LLMRouter(chain: [.codexCLI], transports: [transport])
    let short = LLMSummaryGenerator(router: router, tokenBudget: 8_000)
    let generation = try await short.generate(transcript: "Ada shipped the fix.")
    #expect(generation.summary.title == "Standup")
    #expect(generation.model == "fake-model")
    #expect(generation.provider == "codexCLI")
    let requests = await transport.requests
    #expect(requests.count == 1)
    #expect(requests[0].purpose == .summary)
    #expect(requests[0].jsonSchema == SummaryPrompt.jsonSchema)
    #expect(requests[0].system == SummaryPrompt.system)
    #expect(requests[0].prompt.contains("<transcript>"))

    let longTransport = FakeTransport(
        .grokCLI,
        results: [.success(json)],
        model: "reduce-model",
        prose: "notes"
    )
    let longRouter = LLMRouter(chain: [.grokCLI], transports: [longTransport])
    let long = LLMSummaryGenerator(router: longRouter, tokenBudget: 10)
    let reduced = try await long.generate(transcript: String(repeating: "word ", count: 80))
    #expect(reduced.summary.title == "Standup")
    #expect(reduced.model == "reduce-model")
    #expect(await longTransport.requests.count > 2)
    let prompts = await longTransport.requests.map(\.prompt)
    #expect(prompts.contains { $0.hasPrefix("Combine these partial summaries") })
    let asksForJSON = prompts.contains { prompt in
        prompt.contains("<transcript>") && prompt.contains("Return a JSON object")
    }
    #expect(asksForJSON)
}

@Test func invalidModelOutputIsClassifiedWithoutTheOutput() async throws {
    let transport = FakeTransport(.codexCLI, results: [.success("not json at all")])
    let router = LLMRouter(chain: [.codexCLI], transports: [transport])
    let generator = LLMSummaryGenerator(router: router, tokenBudget: 100)
    do {
        _ = try await generator.generate(transcript: "UNIQUE_TRANSCRIPT_SENTINEL")
        Issue.record("Expected invalid output")
    } catch {
        let classified = SummaryJobError.classify(error)
        #expect(classified.errorClass == "invalidOutput")
        #expect(classified.description.hasPrefix("invalidOutput: "))
        #expect(!classified.description.contains("UNIQUE_TRANSCRIPT_SENTINEL"))
        #expect(!classified.description.contains("not json"))
    }
}

private actor FakeTransport: LLMTransport {
    nonisolated let id: TransportID
    private let model: String?
    private let prose: String?
    private var results: [Result<String, LLMError>]
    private(set) var requests: [LLMRequest] = []

    init(
        _ id: TransportID,
        results: [Result<String, LLMError>],
        model: String? = nil,
        prose: String? = nil
    ) {
        self.id = id
        self.results = results
        self.model = model
        self.prose = prose
    }

    func status() async -> TransportStatus {
        TransportStatus(installed: true, signedIn: true, model: model)
    }

    func run(_ request: LLMRequest) async throws -> LLMResponse {
        requests.append(request)
        if request.jsonSchema == nil, let prose {
            return LLMResponse(text: prose, transport: id, model: model, duration: .zero)
        }
        let result = results.count > 1 ? results.removeFirst() : results[0]
        return LLMResponse(text: try result.get(), transport: id, model: model, duration: .zero)
    }
}
