import Foundation

enum TextTokens {
    /// Letters and digits, matching the separators FTS5's unicode61 tokenizer uses.
    static func words(in text: String) -> [String] {
        var words: [String] = []
        var current = ""
        for character in text {
            let isWord = character.unicodeScalars.allSatisfy {
                CharacterSet.alphanumerics.contains($0)
            }
            if isWord {
                current.append(character)
            } else if !current.isEmpty {
                words.append(current)
                current = ""
            }
        }
        if !current.isEmpty { words.append(current) }
        return words
    }
}
