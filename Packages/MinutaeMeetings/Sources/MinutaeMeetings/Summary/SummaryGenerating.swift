import Foundation
import MinutaeLLM

public protocol SummaryGenerating: Sendable {
    func generate(transcript: String) async throws -> SummaryGeneration
}

/// JSON mode through `LLMRouter`. Transcripts longer than `tokenBudget` × 4
/// characters are reduced with `summarizeLong`, then parsed as JSON.
public struct LLMSummaryGenerator: SummaryGenerating {
    private let router: LLMRouter
    private let tokenBudget: Int
    private let modelFallback: String
    private let timeout: Duration

    public init(
        router: LLMRouter,
        tokenBudget: Int = 8_000,
        modelFallback: String = "llm-router",
        timeout: Duration = .seconds(180)
    ) {
        self.router = router
        self.tokenBudget = tokenBudget
        self.modelFallback = modelFallback
        self.timeout = timeout
    }

    public func generate(transcript: String) async throws -> SummaryGeneration {
        let budget = max(1, tokenBudget)
        if transcript.count <= budget * 4 {
            return try await complete(request(prompt: SummaryPrompt.user(transcript: transcript)))
        }
        var map = request(prompt: SummaryPrompt.mapPreamble)
        map.jsonSchema = nil
        let reduced = try await router.summarizeLong(
            transcript: transcript,
            request: map,
            tokenBudget: budget
        )
        return try await complete(request(prompt: SummaryPrompt.user(transcript: reduced.text)))
    }

    private func request(prompt: String) -> LLMRequest {
        LLMRequest(
            system: SummaryPrompt.system,
            prompt: prompt,
            jsonSchema: SummaryPrompt.jsonSchema,
            timeout: timeout,
            purpose: .summary
        )
    }

    private func complete(_ request: LLMRequest) async throws -> SummaryGeneration {
        let response = try await router.run(request)
        if let summary = Self.decode(response.text) {
            return generation(summary, response: response)
        }
        let summary = try await router.run(request, as: MeetingSummary.self)
        return generation(summary, response: response)
    }

    private func generation(_ summary: MeetingSummary, response: LLMResponse) -> SummaryGeneration {
        SummaryGeneration(
            summary: summary,
            model: response.model ?? modelFallback,
            provider: response.transport.rawValue
        )
    }

    static func decode(_ raw: String) -> MeetingSummary? {
        var cleaned = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if let regex = try? NSRegularExpression(pattern: #"^```json\s*"#, options: [.caseInsensitive]) {
            let range = NSRange(cleaned.startIndex..., in: cleaned)
            cleaned = regex.stringByReplacingMatches(in: cleaned, range: range, withTemplate: "")
        }
        if cleaned.hasSuffix("```") {
            cleaned = String(cleaned.dropLast(3))
        }
        cleaned = cleaned.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let data = cleaned.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(MeetingSummary.self, from: data)
    }
}
