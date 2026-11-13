import Foundation

public enum TranscriptChunker {
    /// Whitespace chunks. `chunkWords` of 200 and `overlapWords` of 20 match the previous app.
    /// Overlap is clamped so the window always advances.
    public static func chunks(of text: String, chunkWords: Int = 200, overlapWords: Int = 20)
        -> [String]
    {
        let words = text.split(whereSeparator: \.isWhitespace).map(String.init)
        if words.isEmpty { return [] }
        let size = max(chunkWords, 1)
        let step = max(size - max(overlapWords, 0), 1)
        var index = 0
        var chunks: [String] = []
        while index < words.count {
            let end = min(index + size, words.count)
            chunks.append(words[index..<end].joined(separator: " "))
            if end == words.count { break }
            index += step
        }
        return chunks
    }
}
