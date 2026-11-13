import Foundation

/// Optional last pass. This package does not import MinutaeLLM.
///
/// The app can adapt its router with `ClosureLLMTextCleanup`. Cleanup stays
/// off until `LocalPostProcessor` is created with `llmEnabled: true`.
public protocol LLMTextCleanup: Sendable {
    func cleanup(_ text: String) async throws -> String
}

public struct TextCleanupFailure: Error, Sendable, Equatable {
    public var message: String

    public init(message: String) {
        self.message = message
    }
}

public struct ClosureLLMTextCleanup: LLMTextCleanup {
    public var clean: @Sendable (String) async throws -> String

    public init(_ clean: @escaping @Sendable (String) async throws -> String) {
        self.clean = clean
    }

    public func cleanup(_ text: String) async throws -> String {
        try await clean(text)
    }
}

public actor FakeLLMTextCleanup: LLMTextCleanup {
    public var result: Result<String, TextCleanupFailure>
    public private(set) var inputs: [String] = []

    public init(result: Result<String, TextCleanupFailure> = .success("")) {
        self.result = result
    }

    public func cleanup(_ text: String) async throws -> String {
        inputs.append(text)
        return try result.get()
    }
}
