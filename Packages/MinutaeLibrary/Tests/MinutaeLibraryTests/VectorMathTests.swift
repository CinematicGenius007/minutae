import Foundation
import Testing

@testable import MinutaeLibrary

@Test func cosineSimilarityMatchesGeometry() {
    #expect(abs(VectorMath.cosineSimilarity([1, 0], [1, 0]) - 1) < 0.00001)
    #expect(abs(VectorMath.cosineSimilarity([1, 0], [0, 1])) < 0.00001)
    #expect(abs(VectorMath.cosineSimilarity([1, 0], [-1, 0]) + 1) < 0.00001)
    #expect(abs(VectorMath.cosineSimilarity([3, 4], [6, 8]) - 1) < 0.00001)
    #expect(VectorMath.cosineSimilarity([1, 0], [1, 0, 0]) == 0)
    #expect(VectorMath.cosineSimilarity([], []) == 0)
    #expect(VectorMath.cosineSimilarity([0, 0], [0, 0]) == 0)
}

@Test func normalizedVectorHasUnitLength() {
    let unit = VectorMath.normalized([3, 4])
    let length = (unit[0] * unit[0] + unit[1] * unit[1]).squareRoot()
    #expect(abs(length - 1) < 0.00001)
    #expect(VectorMath.normalized([0, 0]) == [0, 0])
}

@Test func topKOrdersByScoreThenId() {
    let ranked = VectorMath.topK([("c", Float(0.2)), ("a", 0.9), ("b", 0.9), ("d", -1)], limit: 2)
    #expect(ranked.map(\.id) == ["a", "b"])
    #expect(ranked.map(\.score) == [0.9, 0.9])
    #expect(VectorMath.topK([("a", Float(1))], limit: 0).isEmpty)
    let tied = VectorMath.topK([("b", Float(0.2)), ("a", 0.2)], limit: 5)
    #expect(tied.map(\.id) == ["a", "b"])
}

@Test func vectorBlobRoundTripsNativeFloat32() {
    let original: [Float] = [0.25, -2, 0, 42]
    let data = VectorBlob.encode(original)
    #expect(data.count == 16)
    #expect(VectorBlob.decode(data, dimension: 4) == original)
    #expect(VectorBlob.decode(data, dimension: 3) == nil)
    #expect(VectorBlob.decode(Data("[]".utf8), dimension: 4) == nil)
    #expect(VectorBlob.decode(Data([1, 2, 3]), dimension: 1) == nil)
}

@Test func rankFusionPrefersAnItemPresentInBothLists() {
    let older = SearchHit(
        kind: .meeting, id: "a", title: "A", date: "2026-01-01", snippet: "", score: 0)
    let newer = SearchHit(
        kind: .dictation,
        id: "d",
        title: "D",
        date: "2026-12-01",
        snippet: "",
        score: 0
    )
    let tied = RankFusion.fuse([[older], [newer]], limit: 2)
    #expect(tied.map(\.id) == ["d", "a"])
    let boosted = RankFusion.fuse([[older], [newer], [older]], limit: 2)
    #expect(boosted.first?.id == "a")
    #expect((boosted.first?.score ?? 0) > (boosted.dropFirst().first?.score ?? 0))
}

@Test func rankFusionKeepsHighlightedSnippet() {
    let plain = SearchHit(
        kind: .meeting, id: "a", title: "A", date: "2026-01-01", snippet: "apollo", score: 0)
    let marked = SearchHit(
        kind: .meeting,
        id: "a",
        title: "A",
        date: "2026-01-01",
        snippet: "<mark>apollo</mark>",
        score: 0
    )
    #expect(
        RankFusion.fuse([[plain], [marked]], limit: 1).first?.snippet.contains("<mark>") == true)
    #expect(
        RankFusion.fuse([[marked], [plain]], limit: 1).first?.snippet.contains("<mark>") == true)
}

@Test func hashEmbeddingsAreDeterministicAndLexical() async throws {
    let provider = HashEmbeddingProvider(dimension: 256)
    let first = try await provider.embed(text: "Apollo launch")
    let second = try await provider.embed(text: "apollo   launch")
    #expect(first == second)
    #expect(first.count == 256)
    let budget = try await provider.embed(text: "quarterly budget review hiring forecast")
    let query = try await provider.embed(text: "quarterly budget")
    let garden = try await provider.embed(text: "tomatoes basil water wilted")
    #expect(VectorMath.cosineSimilarity(query, budget) > VectorMath.cosineSimilarity(query, garden))
    let empty = try await provider.embed(text: "...")
    #expect(empty.allSatisfy { $0 == 0 })
}

@Test func transcriptChunkerOverlapsAndTerminates() {
    #expect(TranscriptChunker.chunks(of: "   ").isEmpty)
    #expect(TranscriptChunker.chunks(of: "hello, world") == ["hello, world"])
    #expect(
        TranscriptChunker.chunks(
            of: "one two three four five six seven eight nine ten",
            chunkWords: 4,
            overlapWords: 1
        ) == [
            "one two three four",
            "four five six seven",
            "seven eight nine ten",
        ]
    )
    #expect(
        TranscriptChunker.chunks(of: "one two three four five", chunkWords: 3, overlapWords: 9)
            == ["one two three", "two three four", "three four five"]
    )
}
