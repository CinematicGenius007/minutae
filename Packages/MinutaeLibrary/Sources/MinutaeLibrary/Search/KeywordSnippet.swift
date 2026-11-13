import Foundation

enum KeywordSnippet {
    static func make(text: String, tokens: [String]) -> String {
        let lowered = tokens.map { $0.lowercased() }
        guard let last = lowered.last else { return excerpt(text) }
        let exact = Set(lowered.dropLast())
        let words = text.split(whereSeparator: \.isWhitespace).map(String.init)
        func isHit(_ word: String) -> Bool {
            let keys = TextTokens.words(in: word).map { $0.lowercased() }
            return keys.contains { exact.contains($0) || $0.hasPrefix(last) }
        }
        guard let firstHit = words.firstIndex(where: isHit) else { return excerpt(text) }
        let start = max(0, firstHit - 8)
        let end = min(words.count, firstHit + 12)
        let slice = words[start..<end].map { word in
            isHit(word) ? "<mark>\(word)</mark>" : word
        }.joined(separator: " ")
        var result = slice
        if start > 0 { result = "… " + result }
        if end < words.count { result += " …" }
        return result
    }

    static func excerpt(_ text: String, limit: Int = 180) -> String {
        guard text.count > limit else { return text }
        let end = text.index(text.startIndex, offsetBy: limit)
        return String(text[..<end]) + "…"
    }

    static func dictationTitle(text: String, appName: String?) -> String {
        let line = text.split(whereSeparator: \.isNewline).first.map(String.init) ?? ""
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty { return excerpt(trimmed, limit: 80) }
        let app = appName?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return app.isEmpty ? "Dictation" : app
    }
}
