import Foundation
import MinutaeCore

/// Whether the local Ollama daemon can answer offline.
public enum OllamaReadiness: String, Sendable, Equatable {
    case notInstalled
    case serverDown
    case noModel
    case cloudOnly
    case ready
    /// The daemon answered, but `/api/tags` was not a model list.
    case unknown
}

/// One model from `GET /api/tags`. A cloud model has `remote_host` or `remote_model`.
public struct OllamaModelInfo: Sendable, Equatable {
    public var name: String
    public var isLocal: Bool
    public var remoteHost: String?
    public var remoteModel: String?
    public var digest: String?

    public init(
        name: String,
        isLocal: Bool,
        remoteHost: String? = nil,
        remoteModel: String? = nil,
        digest: String? = nil
    ) {
        self.name = name
        self.isLocal = isLocal
        self.remoteHost = remoteHost
        self.remoteModel = remoteModel
        self.digest = digest
    }
}

/// Daemon reachability plus every installed model. `status()` is the summary of this.
public struct OllamaStatusReport: Sendable, Equatable {
    public var readiness: OllamaReadiness
    public var daemonReachable: Bool
    public var installed: Bool
    public var binaryPath: String?
    public var version: String?
    public var models: [OllamaModelInfo]
    /// Model `run` would use, or the caller's selection when that selection cannot run.
    public var selectedModel: String?
    public var detail: String

    public init(
        readiness: OllamaReadiness,
        daemonReachable: Bool,
        installed: Bool,
        binaryPath: String?,
        version: String?,
        models: [OllamaModelInfo],
        selectedModel: String?,
        detail: String
    ) {
        self.readiness = readiness
        self.daemonReachable = daemonReachable
        self.installed = installed
        self.binaryPath = binaryPath
        self.version = version
        self.models = models
        self.selectedModel = selectedModel
        self.detail = detail
    }

    public var transportStatus: TransportStatus {
        TransportStatus(
            installed: installed,
            binaryPath: binaryPath,
            model: selectedModel,
            version: version,
            detail: detail,
            availableModels: models.map(\.name),
            daemonReachable: daemonReachable
        )
    }
}

/// One NDJSON progress object from `POST /api/pull`.
public struct OllamaPullProgress: Sendable, Equatable {
    public var status: String
    public var digest: String?
    public var completed: Int64?
    public var total: Int64?

    public init(status: String, digest: String? = nil, completed: Int64? = nil, total: Int64? = nil) {
        self.status = status
        self.digest = digest
        self.completed = completed
        self.total = total
    }
}

/// Ollama over the local HTTP API. Nothing is downloaded unless `pullModel` is called.
public struct OllamaTransport: LLMTransport {
    public let id: TransportID = .ollama
    public static let defaultBaseURL = URL(string: "http://127.0.0.1:11434") ?? URL(fileURLWithPath: "/")

    private let requestedModel: String?
    private let baseURL: URL
    private let binaryPath: String?
    private let session: URLSession

    /// `model` is the caller's selection. Pass nil to use the first local model at call time.
    public init(
        model: String? = nil,
        baseURL: URL = OllamaTransport.defaultBaseURL,
        binaryPath: String? = nil,
        session: URLSession? = nil
    ) {
        let trimmed = model?.trimmingCharacters(in: .whitespacesAndNewlines)
        requestedModel = (trimmed?.isEmpty == false) ? trimmed : nil
        self.baseURL = baseURL
        if let binaryPath {
            self.binaryPath = SubprocessRunner.resolveBinary(binaryPath)
        } else {
            self.binaryPath = Self.resolveInstalledBinary()
        }
        self.session = session ?? Self.makeSession()
    }

    public func status() async -> TransportStatus {
        (try? await loadReport())?.transportStatus ?? offlineReport(cancelled: true).transportStatus
    }

    public func ollamaStatus() async -> OllamaStatusReport {
        (try? await loadReport()) ?? offlineReport(cancelled: true)
    }

    public func run(_ request: LLMRequest) async throws -> LLMResponse {
        do {
            let snapshot = try await loadReport(probeTimeout: request.timeout)
            let model = try modelToRun(snapshot)
            let body = try Self.chatBody(request, model: model)
            let clock = ContinuousClock()
            let start = clock.now
            let data = try await send(method: "POST", path: "/api/chat", body: body, timeout: request.timeout)
            let parsed = try Self.parseChat(data)
            return LLMResponse(
                text: parsed.text,
                transport: id,
                model: parsed.model ?? model,
                usage: parsed.usage,
                duration: start.duration(to: clock.now)
            )
        } catch {
            throw mapTransportError(error)
        }
    }

    /// Downloads `name` from the local daemon. The name comes from the caller; this does not pick one.
    public func pullModel(
        _ name: String,
        onProgress: @escaping @Sendable (OllamaPullProgress) -> Void
    ) async throws {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard Self.isAcceptableModelName(trimmed) else {
            throw LLMError.modelRejected("Choose a model name before pulling.")
        }
        let payload: [String: Any] = ["model": trimmed, "stream": true]
        guard JSONSerialization.isValidJSONObject(payload) else {
            throw LLMError.invalidOutput("Could not encode the Ollama pull request.")
        }
        let body = try JSONSerialization.data(withJSONObject: payload)
        let request = try makeRequest(method: "POST", path: "/api/pull", body: body, timeout: .seconds(3600))
        do {
            try await readPull(request, onProgress: onProgress)
        } catch {
            throw mapTransportError(error)
        }
    }

    private func readPull(
        _ request: URLRequest,
        onProgress: @escaping @Sendable (OllamaPullProgress) -> Void
    ) async throws {
        let (bytes, response) = try await bytes(for: request)
        guard let http = response as? HTTPURLResponse else {
            throw LLMError.unavailable("Ollama returned a response that is not HTTP.")
        }
        if !(200..<300).contains(http.statusCode) {
            var collected = Data()
            for try await byte in bytes where collected.count < 8_192 {
                collected.append(byte)
            }
            try Self.throwIfHTTPError(response, body: collected)
            return
        }
        var succeeded = false
        for try await line in bytes.lines {
            let progress = try Self.pullProgress(line)
            if progress.status.isEmpty { continue }
            if progress.status.caseInsensitiveCompare("success") == .orderedSame { succeeded = true }
            onProgress(progress)
        }
        if !succeeded {
            throw LLMError.unavailable("Ollama pull did not finish.")
        }
    }

    private static func pullProgress(_ line: String) throws -> OllamaPullProgress {
        let trimmedLine = line.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmedLine.isEmpty {
            return OllamaPullProgress(status: "")
        }
        guard let data = trimmedLine.data(using: .utf8),
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            throw LLMError.invalidOutput("Ollama pull progress was not JSON.")
        }
        if let error = nonEmpty(object["error"]) {
            throw classifyAPIError(error)
        }
        guard let status = nonEmpty(object["status"]) else {
            return OllamaPullProgress(status: "")
        }
        return OllamaPullProgress(
            status: status,
            digest: nonEmpty(object["digest"]),
            completed: int64Value(object["completed"]),
            total: int64Value(object["total"])
        )
    }

    private func loadReport(probeTimeout: Duration = .seconds(3)) async throws -> OllamaStatusReport {
        try Task.checkCancellation()
        let installed = binaryPath != nil
        guard Self.isLoopback(baseURL) else {
            return OllamaStatusReport(
                readiness: installed ? .serverDown : .notInstalled,
                daemonReachable: false,
                installed: installed,
                binaryPath: binaryPath,
                version: nil,
                models: [],
                selectedModel: requestedModel,
                detail: "Ollama only talks to a loopback address."
            )
        }
        let versionData = try await optionalData(path: "/api/version", timeout: probeTimeout)
        let tags: Data
        do {
            tags = try await send(method: "GET", path: "/api/tags", body: nil, timeout: probeTimeout)
        } catch LLMError.cancelled {
            throw LLMError.cancelled
        } catch {
            return daemonDownReport(installed: installed)
        }
        let version = versionData.flatMap(Self.parseVersion)
        do {
            let models = try Self.parseModels(tags)
            return report(models: models, version: version, installed: installed)
        } catch LLMError.cancelled {
            throw LLMError.cancelled
        } catch {
            return OllamaStatusReport(
                readiness: .unknown,
                daemonReachable: true,
                installed: installed,
                binaryPath: binaryPath,
                version: version,
                models: [],
                selectedModel: requestedModel,
                detail: "Ollama is running, but the model list could not be read."
            )
        }
    }

    private func optionalData(path: String, timeout: Duration) async throws -> Data? {
        do {
            return try await send(method: "GET", path: path, body: nil, timeout: timeout)
        } catch LLMError.cancelled {
            throw LLMError.cancelled
        } catch {
            return nil
        }
    }

    private func report(models: [OllamaModelInfo], version: String?, installed: Bool) -> OllamaStatusReport {
        let local = models.filter(\.isLocal)
        let cloud = models.filter { !$0.isLocal }
        let readiness: OllamaReadiness
        var detail: String
        if models.isEmpty {
            readiness = .noModel
            detail = "Ollama is running and no model is installed. Pull a local model to use it offline."
        } else if local.isEmpty {
            readiness = .cloudOnly
            detail = "Ollama is running, but every installed model is a cloud model (\(Self.names(cloud))). "
                + "A completion would leave this machine."
        } else if cloud.isEmpty {
            readiness = .ready
            detail = "Ollama is running. Local models: \(Self.names(local))."
        } else {
            readiness = .ready
            detail = "Ollama is running. Local models: \(Self.names(local)). Cloud models: \(Self.names(cloud))."
        }
        if let requestedModel {
            detail += Self.selectionNote(requestedModel, models: models)
        }
        if !installed {
            detail += " The ollama binary was not found."
        }
        let selected = requestedModel ?? local.first?.name
        return OllamaStatusReport(
            readiness: readiness,
            daemonReachable: true,
            installed: installed,
            binaryPath: binaryPath,
            version: version,
            models: models,
            selectedModel: selected,
            detail: detail
        )
    }

    private func modelToRun(_ snapshot: OllamaStatusReport) throws -> String {
        guard snapshot.daemonReachable else {
            if !snapshot.installed { throw LLMError.notInstalled }
            throw LLMError.unavailable(snapshot.detail)
        }
        if snapshot.readiness == .unknown {
            throw LLMError.invalidOutput(snapshot.detail)
        }
        if let requestedModel {
            if Self.isCloudName(requestedModel)
                || snapshot.models.contains(where: { $0.name == requestedModel && !$0.isLocal })
            {
                throw LLMError.modelRejected(
                    "\(requestedModel) is a cloud model. A completion would leave this machine."
                )
            }
            guard snapshot.models.contains(where: { $0.name == requestedModel && $0.isLocal }) else {
                throw LLMError.modelRejected(
                    "\(requestedModel) is not installed. Pull it before using Ollama."
                )
            }
            return requestedModel
        }
        if let local = snapshot.models.first(where: \.isLocal) {
            return local.name
        }
        if snapshot.models.isEmpty {
            throw LLMError.unavailable(
                "Ollama is running and no model is installed. Pull a local model to use it offline."
            )
        }
        throw LLMError.modelRejected(
            "Every installed Ollama model is a cloud model. Pull a local model to use Ollama offline."
        )
    }

    private func daemonDownReport(installed: Bool) -> OllamaStatusReport {
        if !installed {
            return OllamaStatusReport(
                readiness: .notInstalled,
                daemonReachable: false,
                installed: false,
                binaryPath: nil,
                version: nil,
                models: [],
                selectedModel: requestedModel,
                detail: "Ollama is not installed."
            )
        }
        return OllamaStatusReport(
            readiness: .serverDown,
            daemonReachable: false,
            installed: true,
            binaryPath: binaryPath,
            version: nil,
            models: [],
            selectedModel: requestedModel,
            detail: Self.unreachableDetail(baseURL)
        )
    }

    private func offlineReport(cancelled: Bool) -> OllamaStatusReport {
        let detail = cancelled ? "Ollama status was cancelled." : Self.unreachableDetail(baseURL)
        return OllamaStatusReport(
            readiness: binaryPath == nil ? .notInstalled : .serverDown,
            daemonReachable: false,
            installed: binaryPath != nil,
            binaryPath: binaryPath,
            version: nil,
            models: [],
            selectedModel: requestedModel,
            detail: detail
        )
    }

    private func send(method: String, path: String, body: Data?, timeout: Duration) async throws -> Data {
        let request = try makeRequest(method: method, path: path, body: body, timeout: timeout)
        let (data, response) = try await data(for: request)
        try Self.throwIfHTTPError(response, body: data)
        return data
    }

    private func makeRequest(method: String, path: String, body: Data?, timeout: Duration) throws -> URLRequest {
        guard Self.isLoopback(baseURL) else {
            throw LLMError.unavailable("Ollama only talks to a loopback address.")
        }
        guard let url = Self.endpoint(baseURL, path) else {
            throw LLMError.unavailable("Ollama base URL is invalid.")
        }
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.timeoutInterval = Self.seconds(timeout)
        request.cachePolicy = .reloadIgnoringLocalCacheData
        if let body {
            request.httpBody = body
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        return request
    }

    private func data(for request: URLRequest) async throws -> (Data, URLResponse) {
        do {
            return try await session.data(for: request)
        } catch {
            throw mapTransportError(error)
        }
    }

    private func bytes(for request: URLRequest) async throws -> (URLSession.AsyncBytes, URLResponse) {
        do {
            return try await session.bytes(for: request)
        } catch {
            throw mapTransportError(error)
        }
    }

    private func mapTransportError(_ error: Error) -> LLMError {
        if let error = error as? LLMError { return error }
        if error is CancellationError { return .cancelled }
        guard let urlError = error as? URLError else {
            return .unavailable("Ollama request failed.")
        }
        switch urlError.code {
        case .cancelled:
            return .cancelled
        case .timedOut:
            return .timeout
        case .cannotConnectToHost, .cannotFindHost, .networkConnectionLost, .notConnectedToInternet,
            .dnsLookupFailed, .resourceUnavailable, .badURL, .unsupportedURL:
            if binaryPath == nil { return .notInstalled }
            return .unavailable(Self.unreachableDetail(baseURL))
        default:
            return .unavailable("Ollama request failed.")
        }
    }

    private static func chatBody(_ request: LLMRequest, model: String) throws -> Data {
        var messages: [[String: String]] = []
        if let system = request.system?.trimmingCharacters(in: .whitespacesAndNewlines), !system.isEmpty {
            messages.append(["role": "system", "content": system])
        }
        messages.append(["role": "user", "content": request.prompt])
        var object: [String: Any] = [
            "format": try formatValue(request.jsonSchema),
            "messages": messages,
            "model": model,
            "stream": false,
        ]
        if let maxOutputTokens = request.maxOutputTokens {
            object["options"] = ["num_predict": maxOutputTokens]
        }
        guard JSONSerialization.isValidJSONObject(object) else {
            throw LLMError.invalidOutput("Could not encode the Ollama request.")
        }
        return try JSONSerialization.data(withJSONObject: object)
    }

    /// `docs/llm-providers.md` asks for `"format": "json"`. A schema object is Ollama's structured form.
    private static func formatValue(_ schema: Data?) throws -> Any {
        guard let schema, !schema.isEmpty else { return "json" }
        let parsed: Any
        do {
            parsed = try JSONSerialization.jsonObject(with: schema)
        } catch {
            throw LLMError.invalidOutput("The JSON schema is not valid JSON.")
        }
        guard parsed is [String: Any] else {
            throw LLMError.invalidOutput("The JSON schema must be an object.")
        }
        return parsed
    }

    private static func parseChat(_ data: Data) throws -> (text: String, model: String?, usage: Usage) {
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw LLMError.invalidOutput("Ollama returned a response that is not a JSON object.")
        }
        if let error = nonEmpty(object["error"]) {
            throw classifyAPIError(error)
        }
        guard let message = object["message"] as? [String: Any], let text = nonEmpty(message["content"]) else {
            throw LLMError.invalidOutput("Ollama returned no message.")
        }
        let usage = Usage(
            inputTokens: intValue(object["prompt_eval_count"]),
            outputTokens: intValue(object["eval_count"])
        )
        return (text, nonEmpty(object["model"]), usage)
    }

    private static func parseModels(_ data: Data) throws -> [OllamaModelInfo] {
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw LLMError.invalidOutput("Ollama model list is not a JSON object.")
        }
        if let error = nonEmpty(object["error"]) {
            throw classifyAPIError(error)
        }
        guard let rows = object["models"] as? [Any] else {
            throw LLMError.invalidOutput("Ollama model list has no models field.")
        }
        var models: [OllamaModelInfo] = []
        for row in rows {
            guard let fields = row as? [String: Any] else { continue }
            let rawName = nonEmpty(fields["name"]) ?? nonEmpty(fields["model"])
            guard let rawName else { continue }
            let remoteHost = nonEmpty(fields["remote_host"])
            let remoteModel = nonEmpty(fields["remote_model"])
            models.append(
                OllamaModelInfo(
                    name: rawName,
                    isLocal: !isCloud(name: rawName, remoteHost: remoteHost, remoteModel: remoteModel),
                    remoteHost: remoteHost,
                    remoteModel: remoteModel,
                    digest: nonEmpty(fields["digest"])
                )
            )
        }
        return models
    }

    private static func parseVersion(_ data: Data) -> String? {
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        return nonEmpty(object["version"])
    }

    private static func isCloud(name: String, remoteHost: String?, remoteModel: String?) -> Bool {
        if remoteHost != nil || remoteModel != nil { return true }
        return isCloudName(name)
    }

    private static func isCloudName(_ name: String) -> Bool {
        let lowered = name.lowercased()
        return lowered.hasSuffix(":cloud") || lowered.hasSuffix("-cloud")
    }

    private static func selectionNote(_ name: String, models: [OllamaModelInfo]) -> String {
        if let match = models.first(where: { $0.name == name }) {
            if !match.isLocal { return " The selected model \(name) is a cloud model." }
            return ""
        }
        if isCloudName(name) { return " The selected model \(name) is a cloud model." }
        return " The selected model \(name) is not installed."
    }

    private static func throwIfHTTPError(_ response: URLResponse, body: Data) throws {
        guard let http = response as? HTTPURLResponse else {
            throw LLMError.unavailable("Ollama returned a response that is not HTTP.")
        }
        guard (200..<300).contains(http.statusCode) else {
            if let object = try? JSONSerialization.jsonObject(with: body) as? [String: Any],
                let error = nonEmpty(object["error"])
            {
                throw classifyAPIError(error)
            }
            if http.statusCode == 404 {
                throw LLMError.modelRejected("Ollama could not find that model.")
            }
            throw LLMError.unavailable("Ollama returned HTTP \(http.statusCode).")
        }
    }

    private static func classifyAPIError(_ message: String) -> LLMError {
        let lower = message.lowercased()
        if lower.contains("rate limit") || lower.contains("too many requests") {
            return .rateLimited(retryAfter: nil)
        }
        if lower.contains("sign in") || lower.contains("not logged") || lower.contains("unauthorized")
            || lower.contains("authentication")
        {
            return .notLoggedIn
        }
        if lower.contains("not found") || lower.contains("not supported") || lower.contains("unknown model") {
            return .modelRejected(clipped(message))
        }
        return .unavailable(clipped(message))
    }

    private static func clipped(_ message: String) -> String {
        if message.count <= 240 { return message }
        return String(message.prefix(240))
    }

    private static func names(_ models: [OllamaModelInfo]) -> String {
        models.map(\.name).joined(separator: ", ")
    }

    private static func isAcceptableModelName(_ name: String) -> Bool {
        if name.isEmpty || name.count > 256 || name.contains("..") || name.hasPrefix("/") || name.hasPrefix(".") {
            return false
        }
        let allowed = CharacterSet(
            charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._:+-/"
        )
        return name.unicodeScalars.allSatisfy { allowed.contains($0) }
    }

    private static func resolveInstalledBinary() -> String? {
        SubprocessRunner.resolveBinary("ollama")
            ?? SubprocessRunner.resolveBinary("/usr/local/bin/ollama")
            ?? SubprocessRunner.resolveBinary("/opt/homebrew/bin/ollama")
            ?? SubprocessRunner.resolveBinary(NSHomeDirectory() + "/.local/bin/ollama")
    }

    private static func makeSession() -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 60
        configuration.timeoutIntervalForResource = 60 * 60
        configuration.waitsForConnectivity = false
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        return URLSession(configuration: configuration)
    }

    private static func isLoopback(_ url: URL) -> Bool {
        guard let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https" else { return false }
        guard url.user == nil, url.password == nil, let host = url.host?.lowercased(), !host.isEmpty else {
            return false
        }
        return host == "localhost" || host == "127.0.0.1" || host == "::1"
    }

    private static func endpoint(_ base: URL, _ path: String) -> URL? {
        guard var components = URLComponents(url: base, resolvingAgainstBaseURL: false) else { return nil }
        components.path = path
        components.query = nil
        components.fragment = nil
        return components.url
    }

    private static func unreachableDetail(_ url: URL) -> String {
        "Ollama is installed, but the server at \(origin(url)) is not reachable."
    }

    private static func origin(_ url: URL) -> String {
        let host = url.host ?? "127.0.0.1"
        guard let port = url.port else { return host }
        return "\(host):\(port)"
    }

    private static func seconds(_ timeout: Duration) -> TimeInterval {
        let parts = timeout.components
        let value = Double(parts.seconds) + Double(parts.attoseconds) / 1e18
        return value < 0.05 ? 0.05 : value
    }

    private static func nonEmpty(_ value: Any?) -> String? {
        guard let text = value as? String else { return nil }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    private static func intValue(_ value: Any?) -> Int? {
        switch value {
        case let number as Int:
            return number
        case let number as Int64 where number <= Int64(Int.max) && number >= Int64(Int.min):
            return Int(number)
        case let number as NSNumber:
            return number.intValue
        default:
            return nil
        }
    }

    private static func int64Value(_ value: Any?) -> Int64? {
        switch value {
        case let number as Int:
            return Int64(number)
        case let number as Int64:
            return number
        case let number as NSNumber:
            return number.int64Value
        default:
            return nil
        }
    }
}
