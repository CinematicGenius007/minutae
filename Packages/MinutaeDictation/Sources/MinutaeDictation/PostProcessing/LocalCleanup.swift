import Foundation

/// Spacing, fillers, the pronoun "I", and terminal punctuation.
///
/// Dictionary replacement sits between `prepare` and `finish`, so a user's
/// spelling wins over these mechanical fixes and is not scanned twice.
enum LocalCleanup {
    private static let fillers: Set<String> = [
        "um", "umm", "ummm", "ummmm",
        "uh", "uhh", "uhhh", "uhhhh",
        "uhm", "uhmm",
    ]

    private static let firstPerson: [String: String] = [
        "i": "I",
        "i'm": "I'm",
        "i'll": "I'll",
        "i've": "I've",
        "i'd": "I'd",
    ]

    static func prepare(_ text: String) -> String {
        var cleaned = TranscriptText.straightenApostrophes(text)
        cleaned = TranscriptText.collapseWhitespace(cleaned)
        cleaned = removeFillers(cleaned)
        cleaned = TranscriptText.collapseWhitespace(cleaned)
        return TranscriptText.removeSpaceBeforePunctuation(cleaned)
    }

    static func finish(_ text: String) -> String {
        var cleaned = TranscriptText.collapseHorizontalWhitespace(text)
        cleaned = cleaned.trimmingCharacters(in: .whitespacesAndNewlines)
        cleaned = TranscriptText.removeSpaceBeforePunctuation(cleaned)
        guard cleaned.contains(where: { $0.isLetter || $0.isNumber }) else { return "" }
        cleaned = fixFirstPerson(cleaned)
        cleaned = sentenceCaseFirstLetter(cleaned)
        return addEndPunctuation(cleaned)
    }

    private static func removeFillers(_ text: String) -> String {
        var result = ""
        var index = text.startIndex
        while index < text.endIndex {
            if let range = TranscriptText.wordRange(in: text, from: index) {
                let word = String(text[range])
                index = range.upperBound
                if fillers.contains(word.lowercased()) {
                    if index < text.endIndex, text[index] == "," {
                        index = text.index(after: index)
                    }
                    continue
                }
                result.append(word)
                continue
            }
            result.append(text[index])
            index = text.index(after: index)
        }
        return result
    }

    private static func fixFirstPerson(_ text: String) -> String {
        var result = ""
        var index = text.startIndex
        while index < text.endIndex {
            if let range = TranscriptText.wordRange(in: text, from: index) {
                let word = String(text[range])
                if let replacement = firstPersonReplacement(word, in: text, range: range) {
                    result.append(replacement)
                } else {
                    result.append(word)
                }
                index = range.upperBound
                continue
            }
            result.append(text[index])
            index = text.index(after: index)
        }
        return result
    }

    private static func firstPersonReplacement(
        _ word: String,
        in text: String,
        range: Range<String.Index>
    ) -> String? {
        let key = word.lowercased()
        if key == "i", isLatinAbbreviation(in: text, range: range) { return nil }
        return firstPerson[key]
    }

    /// Leaves the "i" in "i.e." alone so it is not rewritten as a pronoun.
    private static func isLatinAbbreviation(in text: String, range: Range<String.Index>) -> Bool {
        var index = range.upperBound
        guard index < text.endIndex, text[index] == "." else { return false }
        index = text.index(after: index)
        guard index < text.endIndex else { return false }
        let next = text[index]
        return next == "e" || next == "E"
    }

    /// Uppercases the first letter. A following capital ("iPhone") stays.
    private static func sentenceCaseFirstLetter(_ text: String) -> String {
        guard let first = text.firstIndex(where: \.isLetter) else { return text }
        let character = text[first]
        let upper = String(character).uppercased()
        guard upper != String(character) else { return text }
        let next = text.index(after: first)
        if next < text.endIndex, text[next].isLetter {
            let nextCharacter = text[next]
            if String(nextCharacter) == String(nextCharacter).uppercased() { return text }
        }
        var updated = text
        updated.replaceSubrange(first...first, with: upper)
        return updated
    }

    private static func addEndPunctuation(_ text: String) -> String {
        let closers: Set<Character> = ["\"", "'", "”", "’", ")", "]", "}"]
        var end = text.endIndex
        while end > text.startIndex {
            let previous = text.index(before: end)
            let character = text[previous]
            if character.isWhitespace || closers.contains(character) {
                end = previous
                continue
            }
            break
        }
        guard end > text.startIndex else { return text }
        let mark = text[text.index(before: end)]
        if ".!?…,;:".contains(mark) { return text }
        guard mark.isLetter || mark.isNumber else { return text }
        return text[..<end] + "." + text[end...]
    }
}
