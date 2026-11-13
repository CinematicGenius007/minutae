import Testing
@testable import MinutaeLLM

@Test func classifiesRegisteredPatterns() {
    let classifier = ErrorClassifier(patterns: [
        .init(text: "sign in", error: .notLoggedIn),
        .init(text: "rate limit", error: .rateLimited(retryAfter: .seconds(5))),
    ])
    if case .notLoggedIn = classifier.classify(exitStatus: 1, stdout: "", stderr: "Please SIGN IN") {} else {
        Issue.record("Expected login error")
    }
    if case .rateLimited(let delay) = classifier.classify(exitStatus: 1, stdout: "Rate limit", stderr: "") {
        #expect(delay == .seconds(5))
    } else {
        Issue.record("Expected rate limit")
    }
    #expect(classifier.classify(exitStatus: 0, stdout: "sign in", stderr: "") == nil)
    if case .unavailable = classifier.classify(exitStatus: 2, stdout: "", stderr: "unknown") {} else {
        Issue.record("Expected unavailable")
    }
}
