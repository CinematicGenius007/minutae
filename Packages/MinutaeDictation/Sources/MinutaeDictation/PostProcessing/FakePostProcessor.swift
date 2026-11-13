/// Scripted post-processor for tests that do not care how text is cleaned.
public actor FakePostProcessor: PostProcessor {
    public var result: String?
    public private(set) var texts: [String] = []
    public private(set) var bundleIDs: [String?] = []

    public init(result: String? = nil) {
        self.result = result
    }

    public func process(text: String, bundleID: String?) async -> String {
        texts.append(text)
        bundleIDs.append(bundleID)
        return result ?? text
    }
}
