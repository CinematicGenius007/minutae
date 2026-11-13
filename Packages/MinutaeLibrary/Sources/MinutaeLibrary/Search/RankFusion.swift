enum RankFusion {
    /// Standard reciprocal-rank damping. Rank 1 contributes `1/61`.
    static let damping = 60.0

    static func fuse(_ lists: [[SearchHit]], limit: Int) -> [SearchHit] {
        guard limit > 0 else { return [] }
        var scores: [String: Double] = [:]
        var hits: [String: SearchHit] = [:]
        for list in lists {
            for (index, hit) in list.enumerated() {
                let key = "\(hit.kind.rawValue):\(hit.id)"
                scores[key, default: 0] += 1 / (damping + Double(index + 1))
                hits[key] = merged(existing: hits[key], with: hit)
            }
        }
        let ranked = scores.compactMap { key, score -> SearchHit? in
            guard var hit = hits[key] else { return nil }
            hit.score = score
            return hit
        }
        let sorted = ranked.sorted { lhs, rhs in
            if lhs.score != rhs.score { return lhs.score > rhs.score }
            if lhs.date != rhs.date { return lhs.date > rhs.date }
            if lhs.kind != rhs.kind { return lhs.kind == .meeting }
            return lhs.id < rhs.id
        }
        return Array(sorted.prefix(limit))
    }

    /// Keeps a highlighted snippet when either list has one.
    private static func merged(existing: SearchHit?, with hit: SearchHit) -> SearchHit {
        guard var existing else { return hit }
        if !existing.snippet.contains("<mark>"), hit.snippet.contains("<mark>") {
            existing.snippet = hit.snippet
        }
        if existing.title.isEmpty, !hit.title.isEmpty {
            existing.title = hit.title
        }
        return existing
    }
}
