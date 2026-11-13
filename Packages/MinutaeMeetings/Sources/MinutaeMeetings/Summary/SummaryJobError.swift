import Foundation
import MinutaeLLM

/// Stored by `JobRunner` via `String(describing:)`, which uses `description`.
/// The first token is a stable class name. The message never includes the
/// prompt, transcript, or model output.
public struct SummaryJobError: Error, CustomStringConvertible, Sendable, Equatable {
    public var errorClass: String
    public var message: String

    public init(errorClass: String, message: String) {
        self.errorClass = errorClass
        self.message = message
    }

    public var description: String { "\(errorClass): \(message)" }

    public static func errorClass(fromLastError text: String) -> String? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        let token: String
        if let colon = trimmed.firstIndex(of: ":") {
            token = trimmed[..<colon].trimmingCharacters(in: .whitespaces)
        } else if let space = trimmed.firstIndex(of: " ") {
            token = String(trimmed[..<space])
        } else {
            token = trimmed
        }
        return knownClasses.contains(token) ? token : nil
    }

    static func classify(_ error: any Error) -> SummaryJobError {
        if let error = error as? SummaryJobError { return error }
        if let error = error as? LLMError {
            return SummaryJobError(errorClass: error.stableClass, message: error.summaryMessage)
        }
        if let error = error as? RouterAggregateError {
            let token = error.failures.first?.errorClass ?? "unavailable"
            let errorClass = knownClasses.contains(token) ? token : "unavailable"
            return SummaryJobError(
                errorClass: errorClass,
                message: "Every summary provider failed."
            )
        }
        return SummaryJobError(
            errorClass: "unavailable",
            message: "The summary could not be created."
        )
    }

    static let knownClasses: Set<String> = [
        "notInstalled",
        "notLoggedIn",
        "rateLimited",
        "timeout",
        "cancelled",
        "invalidOutput",
        "modelRejected",
        "unavailable"
    ]
}

extension LLMError {
    var stableClass: String {
        switch self {
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

    var summaryMessage: String {
        switch self {
        case .notInstalled:
            "The summary provider is not installed."
        case .notLoggedIn:
            "Sign in to the summary provider and try again."
        case .rateLimited:
            "The summary provider is rate limited."
        case .timeout:
            "The summary request timed out."
        case .cancelled:
            "The summary was cancelled."
        case .invalidOutput:
            "The summary was not valid."
        case .modelRejected:
            "The summary model was rejected."
        case .unavailable:
            "The summary provider is unavailable."
        }
    }

    var isCancellation: Bool {
        if case .cancelled = self { return true }
        return false
    }
}
