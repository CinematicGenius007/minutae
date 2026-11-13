import Foundation
import MinutaeCore

public struct RouterPolicy: Sendable {
    public var chain: [TransportID]?
    public var timeout: Duration?
    public init(chain: [TransportID]? = nil, timeout: Duration? = nil) {
        self.chain = chain; self.timeout = timeout
    }
}

public struct RouterFailure: Sendable {
    public let transport: TransportID
    public let errorClass: String
}

public struct RouterAggregateError: Error, Sendable {
    public let failures: [RouterFailure]
}

public protocol UsageObserver: Sendable {
    func record(transport: TransportID, purpose: Purpose, duration: Duration, errorClass: String?) async
}

public struct LLMRouter: Sendable {
    private let chain: [TransportID]
    private let registry: [TransportID: any LLMTransport]
    private let policies: [Purpose: RouterPolicy]
    private let observer: (any UsageObserver)?

    public init(chain: [TransportID], transports: [any LLMTransport], policies: [Purpose: RouterPolicy] = [:], observer: (any UsageObserver)? = nil) {
        self.chain = chain
        self.registry = Dictionary(transports.map { ($0.id, $0) }, uniquingKeysWith: { _, last in last })
        self.policies = policies; self.observer = observer
    }

    private func errorName(_ error: LLMError) -> String {
        switch error {
        case .notInstalled: "notInstalled"
        case .notLoggedIn: "notLoggedIn"
        case .rateLimited: "rateLimited"
        case .timeout: "timeout"
        case .cancelled: "cancelled"
        case .invalidOutput: "invalidOutput"
        case .modelRejected: "modelRejected"
        case .unavailable: "unavailable"
        }
    }

    private func call(_ transport: any LLMTransport, _ request: LLMRequest) async throws -> LLMResponse {
        let clock = ContinuousClock(); let start = clock.now
        do {
            try Task.checkCancellation()
            let response = try await transport.run(request)
            try Task.checkCancellation()
            await observer?.record(transport: transport.id, purpose: request.purpose, duration: start.duration(to: clock.now), errorClass: nil)
            return response
        } catch {
            let classified: LLMError = (error is CancellationError || Task.isCancelled) ? .cancelled : (error as? LLMError ?? .unavailable("Transport failed"))
            await observer?.record(transport: transport.id, purpose: request.purpose, duration: start.duration(to: clock.now), errorClass: errorName(classified))
            throw classified
        }
    }

    private func route<T: Sendable>(_ original: LLMRequest, operation: @Sendable (any LLMTransport, LLMRequest) async throws -> T) async throws -> T {
        var request = original
        let policy = policies[request.purpose]
        if let timeout = policy?.timeout { request.timeout = timeout }
        var failures: [RouterFailure] = []
        for id in policy?.chain ?? chain {
            try Task.checkCancellation()
            guard let transport = registry[id] else {
                failures.append(RouterFailure(transport: id, errorClass: "notInstalled")); continue
            }
            let status = await transport.status()
            try Task.checkCancellation()
            if !status.installed { failures.append(RouterFailure(transport: id, errorClass: "notInstalled")); continue }
            if status.signedIn == false { failures.append(RouterFailure(transport: id, errorClass: "notLoggedIn")); continue }
            do { return try await operation(transport, request) }
            catch {
                if error is CancellationError || Task.isCancelled { throw LLMError.cancelled }
                let failure = error as? LLMError ?? .unavailable("Transport failed")
                if case .cancelled = failure { throw failure }
                failures.append(RouterFailure(transport: id, errorClass: errorName(failure)))
                switch failure {
                case .notInstalled, .notLoggedIn, .rateLimited, .timeout, .modelRejected: continue
                case .invalidOutput where original.jsonSchema != nil: continue
                default: throw failure
                }
            }
        }
        throw RouterAggregateError(failures: failures)
    }

    public func run(_ request: LLMRequest) async throws -> LLMResponse {
        try await route(request) { transport, request in try await call(transport, request) }
    }

    public func run<T: Decodable & Sendable>(_ request: LLMRequest, as type: T.Type) async throws -> T {
        var jsonRequest = request
        if jsonRequest.jsonSchema == nil { jsonRequest.jsonSchema = Data() }
        return try await route(jsonRequest) { transport, request in
            let response = try await call(transport, request)
            do { return try JSONDecoder().decode(T.self, from: Data(response.text.utf8)) }
            catch {
                var repair = request
                repair.prompt = "Repair the following JSON to satisfy the requested schema. Return only JSON.\nDecode error: \(error)\nInvalid output:\n\(response.text)"
                let fixed = try await call(transport, repair)
                do { return try JSONDecoder().decode(T.self, from: Data(fixed.text.utf8)) }
                catch { throw LLMError.invalidOutput("JSON validation failed after repair") }
            }
        }
    }

    public static func chunks(_ transcript: String, tokenBudget: Int, overlapTokens: Int = 0) -> [String] {
        let size = max(1, tokenBudget) * 4
        let overlap = min(max(0, overlapTokens) * 4, size - 1)
        let chars = Array(transcript)
        var result: [String] = []; var start = 0
        while start < chars.count {
            let end = min(chars.count, start + size)
            result.append(String(chars[start..<end]))
            if end == chars.count { break }
            start = end - overlap
        }
        return result
    }

    public func summarizeLong(transcript: String, request: LLMRequest, tokenBudget: Int, overlapTokens: Int = 0) async throws -> LLMResponse {
        let parts = Self.chunks(transcript, tokenBudget: tokenBudget, overlapTokens: overlapTokens)
        var summaries: [String] = []
        for part in parts {
            var map = request; map.prompt = "\(request.prompt)\n\n\(part)"
            summaries.append(try await run(map).text)
        }
        var reduce = request
        reduce.prompt = "Combine these partial summaries into a coherent summary:\n\n" + summaries.joined(separator: "\n\n")
        return try await run(reduce)
    }
}
