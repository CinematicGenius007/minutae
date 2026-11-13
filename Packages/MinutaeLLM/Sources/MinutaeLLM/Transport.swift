import Foundation
import MinutaeCore

public enum Purpose: String, Codable, Sendable { case summary, title, cleanup, ask, test }

public struct Usage: Codable, Sendable, Equatable {
    public var inputTokens: Int?
    public var outputTokens: Int?
    public var costUSD: Double?
    public init(inputTokens: Int? = nil, outputTokens: Int? = nil, costUSD: Double? = nil) {
        self.inputTokens = inputTokens; self.outputTokens = outputTokens; self.costUSD = costUSD
    }
}

public struct TransportStatus: Sendable, Equatable {
    public var installed: Bool
    public var binaryPath: String?
    public var signedIn: Bool?
    public var model: String?
    public var version: String?
    public var detail: String?
    public init(installed: Bool, binaryPath: String? = nil, signedIn: Bool? = nil, model: String? = nil, version: String? = nil, detail: String? = nil) {
        self.installed = installed; self.binaryPath = binaryPath; self.signedIn = signedIn
        self.model = model; self.version = version; self.detail = detail
    }
}

public struct LLMRequest: Sendable {
    public var system: String?
    public var prompt: String
    public var jsonSchema: Data?
    public var maxOutputTokens: Int?
    public var timeout: Duration
    public var purpose: Purpose
    public init(system: String? = nil, prompt: String, jsonSchema: Data? = nil, maxOutputTokens: Int? = nil, timeout: Duration, purpose: Purpose) {
        self.system = system; self.prompt = prompt; self.jsonSchema = jsonSchema
        self.maxOutputTokens = maxOutputTokens; self.timeout = timeout; self.purpose = purpose
    }
}

public struct LLMResponse: Sendable {
    public var text: String
    public var transport: TransportID
    public var model: String?
    public var usage: Usage?
    public var duration: Duration
    public init(text: String, transport: TransportID, model: String? = nil, usage: Usage? = nil, duration: Duration) {
        self.text = text; self.transport = transport; self.model = model; self.usage = usage; self.duration = duration
    }
}

public protocol LLMTransport: Sendable {
    var id: TransportID { get }
    func status() async -> TransportStatus
    func run(_ request: LLMRequest) async throws -> LLMResponse
}
