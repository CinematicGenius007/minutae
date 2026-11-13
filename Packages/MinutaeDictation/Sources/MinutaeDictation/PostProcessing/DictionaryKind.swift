/// `dictionary.kind`. Anything else is ignored and never inserted.
public enum DictionaryKind: String, Codable, Sendable, CaseIterable {
    case word
    case snippet
}
