import Foundation

public struct SummaryActionItem: Codable, Sendable, Equatable {
    public var text: String
    public var owner: String?
    public var due: String?

    public init(text: String, owner: String? = nil, due: String? = nil) {
        self.text = text
        self.owner = Self.nonEmpty(owner)
        self.due = Self.nonEmpty(due)
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let text = try? container.decode(String.self) {
            self.init(text: text)
            return
        }
        let object = try container.decode(Payload.self)
        self.init(text: object.text, owner: object.owner, due: object.due)
    }

    private struct Payload: Decodable {
        var text: String
        var owner: String?
        var due: String?
    }

    private enum CodingKeys: String, CodingKey {
        case text, owner, due
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(text, forKey: .text)
        try container.encodeIfPresent(owner, forKey: .owner)
        try container.encodeIfPresent(due, forKey: .due)
    }

    /// Checklist line. `text` is unchanged when owner and due are absent or
    /// already written into it, so a legacy string stays byte-for-byte.
    public var checklistText: String {
        var line = text
        if let owner, !contains(line, owner) {
            line = "\(owner): \(line)"
        }
        if let due, !contains(line, due) {
            line = "\(line) (due \(due))"
        }
        return line
    }

    private static func nonEmpty(_ value: String?) -> String? {
        guard let value, !value.isEmpty else { return nil }
        return value
    }

    private func contains(_ line: String, _ part: String) -> Bool {
        line.range(of: part, options: .caseInsensitive) != nil
    }
}

public struct MeetingSummary: Codable, Sendable, Equatable {
    public var title: String
    public var summary: [String]
    public var keyDecisions: [String]
    public var actionItems: [SummaryActionItem]
    public var participants: [String]

    public init(
        title: String,
        summary: [String],
        keyDecisions: [String],
        actionItems: [SummaryActionItem],
        participants: [String]
    ) {
        self.title = title
        self.summary = summary
        self.keyDecisions = keyDecisions
        self.actionItems = actionItems
        self.participants = participants
    }
}

public struct SummaryGeneration: Sendable, Equatable {
    public var summary: MeetingSummary
    public var model: String
    public var provider: String

    public init(summary: MeetingSummary, model: String, provider: String) {
        self.summary = summary
        self.model = model
        self.provider = provider
    }
}
