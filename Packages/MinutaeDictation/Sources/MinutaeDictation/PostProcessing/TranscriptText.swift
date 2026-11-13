import Foundation

/// Shared idea of a word for cleanup and dictionary matching.
enum TranscriptText {
    static func straightenApostrophes(_ text: String) -> String {
        text.replacingOccurrences(of: "’", with: "'")
            .replacingOccurrences(of: "‘", with: "'")
    }

    /// Turns every whitespace run into a single space and trims the ends.
    static func collapseWhitespace(_ text: String) -> String {
        text.split(whereSeparator: \.isWhitespace).joined(separator: " ")
    }

    /// Collapses spaces and tabs. Line breaks from a snippet stay put.
    static func collapseHorizontalWhitespace(_ text: String) -> String {
        var result = ""
        var pendingSpace = false
        for character in text {
            if character == " " || character == "\t" {
                pendingSpace = !result.isEmpty && result.last != "\n"
                continue
            }
            if character == "\n" {
                pendingSpace = false
                result.append(character)
                continue
            }
            if pendingSpace {
                result.append(" ")
                pendingSpace = false
            }
            result.append(character)
        }
        return result
    }

    static func removeSpaceBeforePunctuation(_ text: String) -> String {
        var result = ""
        for character in text {
            if isTightPunctuation(character), result.last == " " {
                result.removeLast()
            }
            result.append(character)
        }
        return result
    }

    static func normalizedPhrase(_ text: String) -> String {
        straightenApostrophes(text)
            .split(whereSeparator: \.isWhitespace)
            .joined(separator: " ")
    }

    static func wordRange(in text: String, from index: String.Index) -> Range<String.Index>? {
        guard index < text.endIndex else { return nil }
        guard isWordStart(text[index]) else { return nil }
        var end = text.index(after: index)
        while end < text.endIndex {
            let character = text[end]
            if character.isLetter || character.isNumber {
                end = text.index(after: end)
                continue
            }
            if isJoiner(character) {
                let afterJoiner = text.index(after: end)
                if afterJoiner < text.endIndex {
                    let next = text[afterJoiner]
                    if next.isLetter || next.isNumber {
                        end = text.index(after: afterJoiner)
                        continue
                    }
                }
            }
            break
        }
        return index..<end
    }

    static func isWholePhrase(_ text: String, range: Range<String.Index>) -> Bool {
        if range.lowerBound > text.startIndex {
            let before = text[text.index(before: range.lowerBound)]
            if isWordCharacter(before) { return false }
        }
        if range.upperBound < text.endIndex {
            if isWordCharacter(text[range.upperBound]) { return false }
        }
        return true
    }

    static func isWordCharacter(_ character: Character) -> Bool {
        character.isLetter || character.isNumber || isJoiner(character)
    }

    static func isJoiner(_ character: Character) -> Bool {
        character == "'" || character == "-"
    }

    private static func isWordStart(_ character: Character) -> Bool {
        character.isLetter || character.isNumber
    }

    private static func isTightPunctuation(_ character: Character) -> Bool {
        ".,;:!?…".contains(character)
    }
}
