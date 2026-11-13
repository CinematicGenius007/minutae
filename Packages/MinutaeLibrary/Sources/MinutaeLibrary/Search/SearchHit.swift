public struct SearchHit: Sendable, Equatable {
    public enum Kind: String, Sendable, Equatable {
        case meeting
        case dictation
    }

    public var kind: Kind
    public var id: String
    public var title: String
    /// `started_at` as stored: an ISO-8601 UTC string.
    public var date: String
    public var snippet: String
    public var score: Double

    public init(kind: Kind, id: String, title: String, date: String, snippet: String, score: Double)
    {
        self.kind = kind
        self.id = id
        self.title = title
        self.date = date
        self.snippet = snippet
        self.score = score
    }
}
