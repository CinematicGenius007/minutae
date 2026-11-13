import Accelerate
import Foundation

enum VectorMath {
    static func cosineSimilarity(_ lhs: [Float], _ rhs: [Float]) -> Float {
        guard lhs.count == rhs.count, !lhs.isEmpty else { return 0 }
        let dot = vDSP.dot(lhs, rhs)
        let denom = sqrt(vDSP.sumOfSquares(lhs) * vDSP.sumOfSquares(rhs))
        guard denom > 0 else { return 0 }
        let value = dot / denom
        return value.isFinite ? value : 0
    }

    static func normalized(_ vector: [Float]) -> [Float] {
        let norm = sqrt(vDSP.sumOfSquares(vector))
        guard norm > 0 else { return vector }
        var result = [Float](repeating: 0, count: vector.count)
        vDSP.multiply(1 / norm, vector, result: &result)
        return result
    }

    /// Full sort. The library is brute-force over one person's chunks, so a heap is unnecessary.
    static func topK<ID: Comparable>(
        _ items: [(id: ID, score: Float)],
        limit: Int
    ) -> [(id: ID, score: Float)] {
        guard limit > 0 else { return [] }
        let sorted = items.sorted { lhs, rhs in
            if lhs.score != rhs.score { return lhs.score > rhs.score }
            return lhs.id < rhs.id
        }
        return Array(sorted.prefix(limit))
    }
}

/// Native-endian Float32. A row is ignored unless its byte count is `dimension × 4`,
/// which drops the old JSON embeddings without parsing them.
enum VectorBlob {
    static func encode(_ vector: [Float]) -> Data {
        vector.withUnsafeBufferPointer { Data(buffer: $0) }
    }

    static func decode(_ data: Data, dimension: Int) -> [Float]? {
        let byteCount = dimension * MemoryLayout<Float>.size
        guard dimension > 0, data.count == byteCount else { return nil }
        var vector = [Float](repeating: 0, count: dimension)
        vector.withUnsafeMutableBytes { destination in
            _ = data.copyBytes(to: destination)
        }
        return vector
    }
}
