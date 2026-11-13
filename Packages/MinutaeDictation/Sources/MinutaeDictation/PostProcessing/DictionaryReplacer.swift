import Foundation
import MinutaeCore

/// Whole-phrase dictionary substitution on text that is already one line.
///
/// Longer phrases win. For the same span, an app row beats a global row,
/// then a snippet beats a word. The replacement is inserted as stored and
/// is not searched again.
enum DictionaryReplacer {
    private typealias Span = Range<String.Index>

    private struct Candidate {
        var range: Span
        var replacement: String
        var length: Int
        var scoped: Bool
        var snippet: Bool
        var id: String
    }

    static func apply(
        _ text: String,
        entries: [DictionaryEntry],
        bundleID: String?
    ) -> String {
        let applicable = entries.filter { DictionaryScope.applies($0, bundleID: bundleID) }
        var candidates: [Candidate] = []
        for entry in applicable {
            let needle = TranscriptText.normalizedPhrase(entry.term)
            let replacement = trimHorizontal(entry.replacement)
            for range in ranges(of: needle, in: text) {
                candidates.append(
                    Candidate(
                        range: range,
                        replacement: replacement,
                        length: text.distance(from: range.lowerBound, to: range.upperBound),
                        scoped: entry.scope_bundle_id != nil,
                        snippet: entry.kind == DictionaryKind.snippet.rawValue,
                        id: entry.id
                    )
                )
            }
        }
        let accepted = nonOverlapping(candidates)
        return render(text, matches: accepted)
    }

    private static func ranges(of needle: String, in text: String) -> [Span] {
        guard !needle.isEmpty else { return [] }
        var found: [Range<String.Index>] = []
        var start = text.startIndex
        while start < text.endIndex {
            let rest = start..<text.endIndex
            guard let range = text.range(of: needle, options: [.caseInsensitive], range: rest)
            else {
                break
            }
            if TranscriptText.isWholePhrase(text, range: range) {
                found.append(range)
                start = range.upperBound
            } else {
                start = text.index(after: range.lowerBound)
            }
        }
        return found
    }

    private static func nonOverlapping(_ candidates: [Candidate]) -> [Candidate] {
        let ordered = candidates.sorted { left, right in
            if left.length != right.length { return left.length > right.length }
            if left.scoped != right.scoped { return left.scoped && !right.scoped }
            if left.snippet != right.snippet { return left.snippet && !right.snippet }
            if left.range.lowerBound != right.range.lowerBound {
                return left.range.lowerBound < right.range.lowerBound
            }
            return left.id < right.id
        }
        var accepted: [Candidate] = []
        for candidate in ordered {
            let hit = accepted.contains { overlaps($0.range, candidate.range) }
            if !hit { accepted.append(candidate) }
        }
        return accepted.sorted { $0.range.lowerBound < $1.range.lowerBound }
    }

    private static func overlaps(_ left: Span, _ right: Span) -> Bool {
        left.lowerBound < right.upperBound && right.lowerBound < left.upperBound
    }

    private static func render(_ text: String, matches: [Candidate]) -> String {
        var result = ""
        var cursor = text.startIndex
        for match in matches {
            result.append(contentsOf: text[cursor..<match.range.lowerBound])
            result.append(match.replacement)
            cursor = match.range.upperBound
        }
        result.append(contentsOf: text[cursor...])
        return result
    }

    private static func trimHorizontal(_ text: String) -> String {
        var value = Substring(text)
        while let first = value.first, first == " " || first == "\t" {
            value = value.dropFirst()
        }
        while let last = value.last, last == " " || last == "\t" {
            value = value.dropLast()
        }
        return String(value)
    }
}
