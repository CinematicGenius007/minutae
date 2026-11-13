import Foundation
import GRDB
import MinutaeCore

/// Keyword and semantic search over meetings and dictations.
///
/// Meetings use the existing `transcripts_fts` table. Dictations have no FTS table in schema
/// v1/v2, so they are matched in process with the same tokens. Embeddings are stored only for
/// meetings, because `embeddings.meeting_id` cannot point at a dictation. The latest transcript
/// wins. `search` drops semantic ranking when embedding assets are not installed.
public struct SearchService: Sendable {
    private let database: MinutaeDatabase
    private let provider: any EmbeddingProvider
    private static let candidateLimit = 100

    public init(database: MinutaeDatabase, embeddings provider: any EmbeddingProvider) {
        self.database = database
        self.provider = provider
    }

    /// Replaces stored chunks for one meeting. Reads the latest `transcripts.raw_text`.
    public func indexMeeting(
        _ id: String,
        chunkWords: Int = 200,
        overlapWords: Int = 20
    ) async throws {
        guard let text = try latestTranscript(meetingID: id) else {
            try deleteEmbeddings(meetingID: id)
            return
        }
        let chunks = TranscriptChunker.chunks(
            of: text,
            chunkWords: chunkWords,
            overlapWords: overlapWords
        )
        if chunks.isEmpty {
            try deleteEmbeddings(meetingID: id)
            return
        }
        var encoded: [(String, Data)] = []
        encoded.reserveCapacity(chunks.count)
        for chunk in chunks {
            let vector = try await provider.embed(text: chunk)
            guard vector.count == provider.dimension else {
                throw SearchError.dimensionMismatch(
                    expected: provider.dimension, actual: vector.count)
            }
            encoded.append((chunk, VectorBlob.encode(vector)))
        }
        let stored = encoded
        try await database.pool.write { connection in
            try connection.execute(
                sql: "DELETE FROM embeddings WHERE meeting_id = ?", arguments: [id])
            for (index, pair) in stored.enumerated() {
                try connection.execute(
                    sql: """
                        INSERT INTO embeddings (id, meeting_id, chunk_text, chunk_index, vector)
                        VALUES (?, ?, ?, ?, ?)
                        """,
                    arguments: [
                        UUID().uuidString.lowercased(),
                        id,
                        pair.0,
                        index,
                        pair.1,
                    ]
                )
            }
        }
    }

    /// Reciprocal-rank fusion of the FTS meeting list and the dictation list.
    public func keywordSearch(query: String, limit: Int = 10) throws -> [SearchHit] {
        guard limit > 0, FTSQuery.matchExpression(for: query) != nil else { return [] }
        return RankFusion.fuse(
            [try meetingHits(query: query), try dictationHits(query: query)],
            limit: limit
        )
    }

    /// Cosine similarity of the query embedding against stored meeting chunks.
    public func semanticSearch(query: String, limit: Int = 10) async throws -> [SearchHit] {
        guard limit > 0, !FTSQuery.tokens(in: query).isEmpty else { return [] }
        return try await semanticHits(query: query, limit: limit)
    }

    /// Fuses keyword meetings, keyword dictations, and semantic meetings.
    /// Missing embedding assets leave the keyword ranking in place.
    public func search(query: String, limit: Int = 10) async throws -> [SearchHit] {
        guard limit > 0, FTSQuery.matchExpression(for: query) != nil else { return [] }
        let meetings = try meetingHits(query: query)
        let dictations = try dictationHits(query: query)
        let semantic: [SearchHit]
        do {
            semantic = try await semanticHits(query: query, limit: Self.candidateLimit)
        } catch SearchError.assetsUnavailable {
            semantic = []
        }
        return RankFusion.fuse([meetings, dictations, semantic], limit: limit)
    }

    private func latestTranscript(meetingID: String) throws -> String? {
        try database.pool.read { connection in
            try String.fetchOne(
                connection,
                sql: """
                    SELECT raw_text FROM transcripts
                    WHERE meeting_id = ?
                    ORDER BY created_at DESC
                    LIMIT 1
                    """,
                arguments: [meetingID]
            )
        }
    }

    private func deleteEmbeddings(meetingID: String) throws {
        try database.pool.write { connection in
            try connection.execute(
                sql: "DELETE FROM embeddings WHERE meeting_id = ?", arguments: [meetingID])
        }
    }

    private func meetingHits(query: String) throws -> [SearchHit] {
        guard let match = FTSQuery.matchExpression(for: query) else { return [] }
        let rows = try database.pool.read { connection in
            try MeetingMatch.fetchAll(
                connection,
                sql: """
                    SELECT m.id AS id,
                           m.title AS title,
                           m.started_at AS startedAt,
                           COALESCE(
                             snippet(transcripts_fts, 0, '<mark>', '</mark>', '…', 20),
                             ''
                           ) AS snippet
                    FROM transcripts_fts
                    JOIN transcripts t ON t.rowid = transcripts_fts.rowid
                    JOIN meetings m ON m.id = t.meeting_id
                    WHERE transcripts_fts MATCH ?
                    ORDER BY bm25(transcripts_fts), m.started_at DESC, m.id
                    LIMIT ?
                    """,
                arguments: [match, Self.candidateLimit]
            )
        }
        var seen: Set<String> = []
        var hits: [SearchHit] = []
        for row in rows where seen.insert(row.id).inserted {
            hits.append(
                SearchHit(
                    kind: .meeting,
                    id: row.id,
                    title: row.title,
                    date: row.startedAt,
                    snippet: row.snippet,
                    score: 0
                )
            )
        }
        return hits
    }

    /// Dictations are not in `transcripts_fts`, so this scans `final_text` (or `raw_text`).
    private func dictationHits(query: String) throws -> [SearchHit] {
        let tokens = FTSQuery.tokens(in: query)
        guard !tokens.isEmpty else { return [] }
        let rows = try database.pool.read { connection in
            try DictationRow.fetchAll(
                connection,
                sql: """
                    SELECT id,
                           final_text AS finalText,
                           raw_text AS rawText,
                           target_app_name AS targetAppName,
                           started_at AS startedAt
                    FROM dictations
                    """
            )
        }
        let matched = rows.compactMap { row -> DictationMatch? in
            let body = row.finalText.isEmpty ? row.rawText : row.finalText
            guard Self.matches(body, tokens: tokens) else { return nil }
            let wordCount = body.split(whereSeparator: \.isWhitespace).count
            return DictationMatch(row: row, wordCount: wordCount, body: body)
        }.sorted { lhs, rhs in
            if lhs.wordCount != rhs.wordCount { return lhs.wordCount < rhs.wordCount }
            if lhs.row.startedAt != rhs.row.startedAt {
                return lhs.row.startedAt > rhs.row.startedAt
            }
            return lhs.row.id < rhs.row.id
        }
        return matched.prefix(Self.candidateLimit).map { match in
            SearchHit(
                kind: .dictation,
                id: match.row.id,
                title: KeywordSnippet.dictationTitle(
                    text: match.body, appName: match.row.targetAppName),
                date: match.row.startedAt,
                snippet: KeywordSnippet.make(text: match.body, tokens: tokens),
                score: 0
            )
        }
    }

    private func semanticHits(query: String, limit: Int) async throws -> [SearchHit] {
        let queryVector = try await provider.embed(text: query)
        guard queryVector.count == provider.dimension else {
            throw SearchError.dimensionMismatch(
                expected: provider.dimension, actual: queryVector.count)
        }
        let rows = try await database.pool.read { connection in
            try StoredChunk.fetchAll(
                connection,
                sql: """
                    SELECT e.meeting_id AS meetingId,
                           e.chunk_text AS chunkText,
                           e.vector AS vector,
                           m.title AS title,
                           m.started_at AS startedAt
                    FROM embeddings e
                    JOIN meetings m ON m.id = e.meeting_id
                    """
            )
        }
        var best: [String: BestChunk] = [:]
        for row in rows {
            guard let vector = VectorBlob.decode(row.vector, dimension: provider.dimension) else {
                continue
            }
            let score = VectorMath.cosineSimilarity(queryVector, vector)
            if let current = best[row.meetingId], current.score >= score { continue }
            best[row.meetingId] = BestChunk(
                title: row.title,
                date: row.startedAt,
                snippet: KeywordSnippet.excerpt(row.chunkText),
                score: score
            )
        }
        let ranked = VectorMath.topK(
            best.map { (id: $0.key, score: $0.value.score) },
            limit: limit
        )
        return ranked.compactMap { item in
            guard let chunk = best[item.id] else { return nil }
            return SearchHit(
                kind: .meeting,
                id: item.id,
                title: chunk.title,
                date: chunk.date,
                snippet: chunk.snippet,
                score: Double(item.score)
            )
        }
    }

    private static func matches(_ text: String, tokens: [String]) -> Bool {
        let words = TextTokens.words(in: text).map { $0.lowercased() }
        guard let last = tokens.last?.lowercased() else { return false }
        for token in tokens.dropLast() where !words.contains(token.lowercased()) {
            return false
        }
        return words.contains { $0.hasPrefix(last) }
    }
}

private struct MeetingMatch: Decodable, FetchableRecord, Sendable {
    var id: String
    var title: String
    var startedAt: String
    var snippet: String
}

private struct DictationRow: Decodable, FetchableRecord, Sendable {
    var id: String
    var finalText: String
    var rawText: String
    var targetAppName: String?
    var startedAt: String
}

private struct DictationMatch {
    var row: DictationRow
    var wordCount: Int
    var body: String
}

private struct StoredChunk: Decodable, FetchableRecord, Sendable {
    var meetingId: String
    var chunkText: String
    var vector: Data
    var title: String
    var startedAt: String
}

private struct BestChunk {
    var title: String
    var date: String
    var snippet: String
    var score: Float
}
