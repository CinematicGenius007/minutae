import Foundation

/// Turns raw user text into an FTS5 `MATCH` expression.
///
/// Every token is a quoted phrase, so words such as `AND`, `OR`, `NEAR`, and `*` stay
/// literals. Internal quotes are doubled. Only the last token is a prefix query.
enum FTSQuery {
    static func tokens(in query: String) -> [String] {
        TextTokens.words(in: query.replacingOccurrences(of: "\0", with: ""))
    }

    static func matchExpression(for query: String) -> String? {
        let tokens = tokens(in: query)
        guard !tokens.isEmpty else { return nil }
        return tokens.enumerated().map { index, token in
            quotedToken(token, prefix: index == tokens.count - 1)
        }.joined(separator: " ")
    }

    static func quotedToken(_ token: String, prefix: Bool) -> String {
        let escaped = token.replacingOccurrences(of: "\"", with: "\"\"")
        let quoted = "\"\(escaped)\""
        return prefix ? quoted + "*" : quoted
    }
}
