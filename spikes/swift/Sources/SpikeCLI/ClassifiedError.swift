import Foundation

enum ClassifiedError: String, Codable {
    case notInstalled = "not_installed"
    case notLoggedIn = "not_logged_in"
    case rateLimited = "rate_limited"
    case timeout
    case cancelled
    case invalidOutput = "invalid_output"
    case unknown
}

struct ProbeFailure: Error, CustomStringConvertible {
    let code: ClassifiedError
    let detail: String

    var description: String { "\(code.rawValue): \(detail)" }
}
