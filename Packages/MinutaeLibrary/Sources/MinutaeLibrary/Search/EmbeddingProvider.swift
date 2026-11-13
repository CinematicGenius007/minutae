import Foundation
import NaturalLanguage

public enum SearchError: Error, Equatable, Sendable {
    case assetsUnavailable
    case dimensionMismatch(expected: Int, actual: Int)
}

public protocol EmbeddingProvider: Sendable {
    var dimension: Int { get }
    func embed(text: String) async throws -> [Float]
}

/// Deterministic bag-of-words vectors for tests and for ranking without a model download.
/// Each token adds 1 to one bucket of a stable FNV-1a hash. Weights stay positive so two
/// tokens in a short query cannot cancel each other.
public struct HashEmbeddingProvider: EmbeddingProvider {
    public let dimension: Int

    public init(dimension: Int = 256) {
        self.dimension = max(dimension, 1)
    }

    public func embed(text: String) async throws -> [Float] {
        var vector = [Float](repeating: 0, count: dimension)
        for token in TextTokens.words(in: text) {
            let bucket = Int(FNV1a.hash(token.lowercased()) % UInt64(dimension))
            vector[bucket] += 1
        }
        return VectorMath.normalized(vector)
    }
}

enum FNV1a {
    static func hash(_ text: String) -> UInt64 {
        var hash: UInt64 = 14_695_981_039_346_656_037
        for byte in text.utf8 {
            hash ^= UInt64(byte)
            hash &*= 1_099_511_628_211
        }
        return hash
    }
}

/// On-device contextual embeddings. Mean-pools subword vectors from `NLContextualEmbedding`.
/// Does not download assets. `embed` throws `assetsUnavailable` when they are not on the Mac.
public actor ContextualEmbeddingProvider: EmbeddingProvider {
    public nonisolated let dimension: Int
    private let model: NLContextualEmbedding
    private var didLoad = false

    public init?(language: NLLanguage = .english) {
        guard let model = NLContextualEmbedding(language: language) else { return nil }
        self.model = model
        dimension = model.dimension
    }

    public func assetsAvailable() -> Bool {
        model.hasAvailableAssets
    }

    public func embed(text: String) async throws -> [Float] {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return [Float](repeating: 0, count: dimension) }
        guard model.hasAvailableAssets else { throw SearchError.assetsUnavailable }
        if !didLoad {
            try model.load()
            didLoad = true
        }
        let result = try model.embeddingResult(for: trimmed, language: nil)
        return pooled(result, text: trimmed)
    }

    private func pooled(_ result: NLContextualEmbeddingResult, text: String) -> [Float] {
        var sums = [Double](repeating: 0, count: dimension)
        var count = 0
        var index = text.startIndex
        while index < text.endIndex {
            guard let (tokenVector, tokenRange) = result.tokenVector(at: index) else { break }
            if tokenVector.count == dimension {
                for (offset, value) in tokenVector.enumerated() {
                    sums[offset] += value
                }
                count += 1
            }
            if tokenRange.upperBound <= index { break }
            index = tokenRange.upperBound
        }
        guard count > 0 else { return [Float](repeating: 0, count: dimension) }
        let mean = sums.map { Float($0 / Double(count)) }
        return VectorMath.normalized(mean)
    }
}
