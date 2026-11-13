import Foundation
import MinutaeCore

/// Headless `grok` transport. Flags match the verified set in `docs/llm-providers.md`.
///
/// The prompt is written to a private file (`--prompt-file`) and also passed on stdin.
/// Grok reads the file; `-p` would place the prompt on the command line. Dollar cost is
/// ignored because only the API transport records `costUSD`. Credential directories are
/// never opened.
public struct GrokTransport: LLMTransport {
    public let id: TransportID = .grokCLI

    /// Phrases from grok 1.0.77. Logged-out and rate-limit bodies were not captured in LLM-01.
    static let stderrPatterns: [ErrorClassifier.Pattern] = [
        .init(text: "not signed in", error: .notLoggedIn),
        .init(text: "grok login", error: .notLoggedIn),
        .init(text: "authentication required", error: .notLoggedIn),
        .init(text: "xai credential", error: .notLoggedIn),
        .init(text: "rate limit", error: .rateLimited(retryAfter: nil)),
        .init(text: "too many requests", error: .rateLimited(retryAfter: nil)),
    ]

    static let notInstalledDetail = "Grok CLI was not found on PATH or in known locations."

    private static let classifier = ErrorClassifier().registering(stderrPatterns)
    private let model: String?
    private let binary: String?
    private let runner = SubprocessRunner()

    public init(model: String? = nil, binaryPath: String? = nil) {
        let trimmed = model?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        self.model = trimmed.isEmpty ? nil : trimmed
        self.binary = Self.resolveBinary(override: binaryPath)
    }

    /// PATH first, then known install locations. Only executable files match, never a shell function.
    static func resolveBinary(
        path: String = ProcessInfo.processInfo.environment["PATH"] ?? "",
        home: String = NSHomeDirectory(),
        override: String? = nil
    ) -> String? {
        if let override {
            return SubprocessRunner.resolveBinary(override, path: path)
        }
        if let found = SubprocessRunner.resolveBinary("grok", path: path) {
            return found
        }
        for location in knownLocations(home: home) {
            if let found = SubprocessRunner.resolveBinary(location, path: path) {
                return found
            }
        }
        return nil
    }

    static func knownLocations(home: String) -> [String] {
        let root = URL(fileURLWithPath: home, isDirectory: true)
        let grokHome = root.appendingPathComponent(".grok").appendingPathComponent("bin")
            .appendingPathComponent("grok")
        let local = root.appendingPathComponent(".local").appendingPathComponent("bin")
            .appendingPathComponent("grok")
        return [grokHome.path, local.path, "/opt/homebrew/bin/grok", "/usr/local/bin/grok"]
    }

    public func status() async -> TransportStatus {
        guard let binary else {
            return TransportStatus(
                installed: false, signedIn: nil, model: model, detail: Self.notInstalledDetail)
        }
        return TransportStatus(
            installed: true,
            binaryPath: binary,
            signedIn: nil,
            model: model,
            version: await version(of: binary)
        )
    }

    public func run(_ request: LLMRequest) async throws -> LLMResponse {
        guard let binary else { throw LLMError.notInstalled }
        let schema = try Self.schemaString(request.jsonSchema)
        let prompt = Self.composedPrompt(system: request.system, prompt: request.prompt)
        let scratch = try GrokScratch.make(prompt: prompt)
        defer { scratch.remove() }
        let clock = ContinuousClock()
        let started = clock.now
        let result = try await runner.run(
            binary: binary,
            arguments: Self.arguments(
                promptFile: scratch.promptFile.path,
                workingDirectory: scratch.workingDirectory.path,
                model: model,
                jsonSchema: schema
            ),
            prompt: prompt,
            timeout: request.timeout
        )
        if let error = Self.classifier.classify(
            exitStatus: result.exitStatus, stdout: result.stdout, stderr: result.stderr)
        {
            throw error
        }
        let parsed = try Self.parse(result.stdout, fallbackModel: model)
        return LLMResponse(
            text: parsed.text,
            transport: id,
            model: parsed.model,
            usage: parsed.usage,
            duration: started.duration(to: clock.now)
        )
    }

    private func version(of binary: String) async -> String? {
        guard
            let result = try? await runner.run(
                binary: binary, arguments: ["--version"], prompt: "", timeout: .seconds(5)),
            result.exitStatus == 0
        else { return nil }
        let line = result.stdout.split(whereSeparator: \.isNewline).first.map(String.init)
        let trimmed = line?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return trimmed.isEmpty ? nil : trimmed
    }

    private static func arguments(
        promptFile: String, workingDirectory: String, model: String?, jsonSchema: String?
    ) -> [String] {
        var arguments = [
            "--prompt-file", promptFile,
            "--output-format", "json",
            "--tools", "",
            "--disable-web-search",
            "--no-subagents",
            "--max-turns", "1",
            "--cwd", workingDirectory,
            "--verbatim",
        ]
        if let model {
            arguments += ["--model", model]
        }
        if let jsonSchema {
            arguments += ["--json-schema", jsonSchema]
        }
        return arguments
    }

    private static func composedPrompt(system: String?, prompt: String) -> String {
        guard let system, !system.isEmpty else { return prompt }
        return system + "\n\n" + prompt
    }

    /// No verified flag takes `maxOutputTokens`. A schema uses grok's `--json-schema`.
    private static func schemaString(_ data: Data?) throws -> String? {
        guard let data else { return nil }
        guard let text = String(data: data, encoding: .utf8) else {
            throw LLMError.invalidOutput("JSON schema is not UTF-8.")
        }
        return text
    }

    private static func parse(_ stdout: String, fallbackModel: String?) throws -> GrokParsedOutput {
        let object = try jsonObject(stdout)
        if let error = failure(stopReason: object["stopReason"] as? String) {
            throw error
        }
        return GrokParsedOutput(
            text: try text(from: object),
            model: reportedModel(from: object, fallback: fallbackModel),
            usage: tokenUsage(from: object)
        )
    }

    private static func jsonObject(_ stdout: String) throws -> [String: Any] {
        let trimmed = stdout.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let data = trimmed.data(using: .utf8),
            let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        else { throw LLMError.invalidOutput("Grok returned malformed JSON.") }
        return object
    }

    private static func failure(stopReason: String?) -> LLMError? {
        switch stopReason?.lowercased() {
        case "cancelled":
            return .cancelled
        case "rate_limited", "rate-limited":
            return .rateLimited(retryAfter: nil)
        default:
            return nil
        }
    }

    private static func text(from object: [String: Any]) throws -> String {
        guard let text = object["text"] as? String,
            !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else {
            throw LLMError.invalidOutput("Grok returned no text.")
        }
        return text
    }

    private static func tokenUsage(from object: [String: Any]) -> Usage? {
        guard let counts = object["usage"] as? [String: Any] else { return nil }
        let usage = Usage(
            inputTokens: integer(counts["input_tokens"]),
            outputTokens: integer(counts["output_tokens"]))
        guard usage.inputTokens != nil || usage.outputTokens != nil else { return nil }
        return usage
    }

    private static func reportedModel(from object: [String: Any], fallback: String?) -> String? {
        if let name = object["model"] as? String {
            let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmed.isEmpty { return trimmed }
        }
        if let usage = object["modelUsage"] as? [String: Any], usage.count == 1,
            let name = usage.keys.first, !name.isEmpty
        {
            return name
        }
        return fallback
    }

    private static func integer(_ value: Any?) -> Int? {
        if let number = value as? Int { return number }
        guard let number = value as? Double, number.rounded() == number, number >= Double(Int.min),
            number <= Double(Int.max)
        else {
            return nil
        }
        return Int(number)
    }
}

private struct GrokParsedOutput {
    var text: String
    var model: String?
    var usage: Usage?
}

private struct GrokScratch {
    let root: URL
    let promptFile: URL
    let workingDirectory: URL

    static func make(prompt: String) throws -> GrokScratch {
        let manager = FileManager.default
        let root = manager.temporaryDirectory.appendingPathComponent(
            "minutae-grok-\(UUID().uuidString)", isDirectory: true)
        let workingDirectory = root.appendingPathComponent("cwd", isDirectory: true)
        let promptFile = root.appendingPathComponent("prompt.txt")
        try manager.createDirectory(at: root, withIntermediateDirectories: false)
        try manager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: root.path)
        do {
            try manager.createDirectory(at: workingDirectory, withIntermediateDirectories: false)
            try manager.setAttributes(
                [.posixPermissions: 0o700], ofItemAtPath: workingDirectory.path)
            try Data(prompt.utf8).write(to: promptFile, options: .atomic)
            try manager.setAttributes([.posixPermissions: 0o600], ofItemAtPath: promptFile.path)
        } catch {
            try? manager.removeItem(at: root)
            throw error
        }
        return GrokScratch(root: root, promptFile: promptFile, workingDirectory: workingDirectory)
    }

    func remove() {
        try? FileManager.default.removeItem(at: root)
    }
}
