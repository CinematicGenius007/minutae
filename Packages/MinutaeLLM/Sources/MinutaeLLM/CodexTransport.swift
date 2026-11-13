import Foundation
import MinutaeCore

public struct CodexTransport: LLMTransport {
    public let id: TransportID = .codexCLI
    private let codexModel: String
    private let binary: String?
    private let runner = SubprocessRunner()
    private static let classifier = ErrorClassifier(patterns: [
        .init(text: "not logged in", error: .notLoggedIn),
        .init(text: "sign in", error: .notLoggedIn),
        .init(text: "login required", error: .notLoggedIn),
        .init(text: "rate limit", error: .rateLimited(retryAfter: nil)),
        .init(text: "too many requests", error: .rateLimited(retryAfter: nil)),
        .init(text: "not supported", error: .modelRejected("The selected Codex model is not supported by this account.")),
        .init(text: "model not found", error: .modelRejected("The selected Codex model was not found."))
    ])

    public init(codexModel: String, binaryPath: String? = nil) {
        self.codexModel = codexModel.trimmingCharacters(in: .whitespacesAndNewlines)
        if let binaryPath {
            self.binary = SubprocessRunner.resolveBinary(binaryPath)
        } else {
            self.binary = SubprocessRunner.resolveBinary("codex")
                ?? SubprocessRunner.resolveBinary("/opt/homebrew/bin/codex")
                ?? SubprocessRunner.resolveBinary("/usr/local/bin/codex")
                ?? SubprocessRunner.resolveBinary(NSHomeDirectory() + "/.local/bin/codex")
        }
    }

    public func status() async -> TransportStatus {
        TransportStatus(installed: binary != nil, binaryPath: binary, model: codexModel.isEmpty ? nil : codexModel,
                        detail: codexModel.isEmpty ? "Select a Codex model in settings." : nil)
    }

    public func run(_ request: LLMRequest) async throws -> LLMResponse {
        guard !codexModel.isEmpty else { throw LLMError.modelRejected("Select a Codex model in settings.") }
        guard let binary else { throw LLMError.notInstalled }
        let prompt = [request.system, request.prompt].compactMap { $0 }.joined(separator: "\n\n")
        let clock = ContinuousClock()
        let start = clock.now
        let result = try await runner.run(binary: binary, arguments: ["exec", "--skip-git-repo-check", "--sandbox", "read-only", "--ephemeral", "--ignore-user-config", "--ignore-rules", "--json", "--color", "never", "-C", ".", "-o", "last-message.txt", "--model", codexModel, "-"], prompt: prompt, timeout: request.timeout)
        if let error = Self.classifier.classify(exitStatus: result.exitStatus, stdout: result.stdout, stderr: result.stderr) { throw error }
        var message: String?
        var usage: Usage?
        for line in result.stdout.split(separator: "\n") {
            guard let data = String(line).data(using: .utf8),
                  let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { continue }
            if let item = object["item"] as? [String: Any], item["type"] as? String == "agent_message" {
                message = item["text"] as? String
            }
            if let counts = object["usage"] as? [String: Any] {
                usage = Usage(inputTokens: counts["input_tokens"] as? Int, outputTokens: counts["output_tokens"] as? Int)
            }
        }
        guard let message, !message.isEmpty else { throw LLMError.invalidOutput("Codex returned no agent message.") }
        return LLMResponse(text: message, transport: id, model: codexModel, usage: usage, duration: start.duration(to: clock.now))
    }
}
