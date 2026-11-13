import Foundation
import MinutaeCore

public enum LLMError: Error, Sendable {
    case notInstalled
    case notLoggedIn
    case rateLimited(retryAfter: Duration?)
    case timeout
    case cancelled
    case invalidOutput(String)
    case modelRejected(String)
    case unavailable(String)
}

/// Matches vendor-specific failures without embedding any vendor patterns.
public struct ErrorClassifier: Sendable {
    public struct Pattern: Sendable {
        public let text: String
        public let error: LLMError

        public init(text: String, error: LLMError) {
            self.text = text
            self.error = error
        }
    }

    private let patterns: [Pattern]

    public init(patterns: [Pattern] = []) {
        self.patterns = patterns
    }

    /// Appends patterns after the ones already registered. Earlier patterns still win.
    public func registering(_ additional: [Pattern]) -> ErrorClassifier {
        ErrorClassifier(patterns: patterns + additional)
    }

    /// Returns nil for successful exits. Patterns are matched in order, case-insensitively.
    public func classify(exitStatus: Int32, stdout: String, stderr: String) -> LLMError? {
        guard exitStatus != 0 else { return nil }
        for pattern in patterns where !pattern.text.isEmpty {
            if stdout.localizedCaseInsensitiveContains(pattern.text)
                || stderr.localizedCaseInsensitiveContains(pattern.text) {
                return pattern.error
            }
        }
        return .unavailable("Process exited with status \(exitStatus)")
    }
}
