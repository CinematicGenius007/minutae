import { eq } from "drizzle-orm"
import { cosineSimilarity } from "@/lib/embeddings/embed"
import { formatDate, nanoid } from "@/lib/utils"
import { getDb, getSqlite } from "./client"
import { embeddings, meetings } from "./schema"

// ─── Types ────────────────────────────────────────────────────────────────────

export interface SearchResult {
	meetingId: string
	title: string
	date: string
	excerpt: string
	score?: number
}

// ─── Write ────────────────────────────────────────────────────────────────────

/**
 * Stores a batch of text chunks + their vectors for a given meeting.
 * Replaces any previously stored embeddings for that meeting first.
 */
export async function storeEmbeddings(
	meetingId: string,
	chunks: { text: string; index: number; vector: Float32Array }[],
) {
	const db = await getDb()

	// Clear stale embeddings for this meeting
	await db.delete(embeddings).where(eq(embeddings.meetingId, meetingId))

	for (const chunk of chunks) {
		await db.insert(embeddings).values({
			id: nanoid(),
			meetingId,
			chunkText: chunk.text,
			chunkIndex: chunk.index,
			vector: JSON.stringify(Array.from(chunk.vector)),
		})
	}
}

// ─── Keyword search (FTS5) ────────────────────────────────────────────────────

/**
 * Full-text search over all transcripts using SQLite FTS5.
 * Returns up to `limit` results ordered by relevance rank.
 *
 * The `snippet()` function inserts `<mark>` tags around matched terms.
 */
export async function ftsSearch(query: string, limit = 10): Promise<SearchResult[]> {
	if (!query.trim()) return []

	const sqlite = await getSqlite()

	// Append * for prefix matching so partial words still match
	const ftsQuery = `${query.trim().replace(/['"*]/g, "")}*`

	const rows = await sqlite.select<
		{
			meetingId: string
			title: string
			startedAt: string
			excerpt: string
		}[]
	>(
		`SELECT m.id      AS meetingId,
		        m.title,
		        m.started_at AS startedAt,
		        snippet(transcripts_fts, 0, '<mark>', '</mark>', '…', 20) AS excerpt
		 FROM   transcripts_fts
		 JOIN   transcripts t ON t.rowid = transcripts_fts.rowid
		 JOIN   meetings m    ON m.id    = t.meeting_id
		 WHERE  transcripts_fts MATCH ?
		 ORDER  BY rank
		 LIMIT  ?`,
		[ftsQuery, limit],
	)

	return rows.map((r) => ({
		meetingId: r.meetingId,
		title: r.title,
		date: formatDate(r.startedAt),
		excerpt: r.excerpt,
	}))
}

// ─── Semantic search ─────────────────────────────────────────────────────────

/**
 * Semantic search using cosine similarity over stored embeddings.
 *
 * 1. Embed the query with all-MiniLM-L6-v2.
 * 2. Load all stored chunk vectors from SQLite (in-memory comparison).
 * 3. Keep the best score per meeting; return top `limit` meetings.
 *
 * Falls back gracefully: if no embeddings exist, returns [].
 */
export async function semanticSearch(
	queryVector: Float32Array,
	limit = 10,
): Promise<SearchResult[]> {
	const db = await getDb()

	// Load all chunks with their meeting metadata
	const rows = await db
		.select({
			id: embeddings.id,
			meetingId: embeddings.meetingId,
			chunkText: embeddings.chunkText,
			vector: embeddings.vector,
			title: meetings.title,
			startedAt: meetings.startedAt,
		})
		.from(embeddings)
		.innerJoin(meetings, eq(embeddings.meetingId, meetings.id))

	if (rows.length === 0) return []

	const qv = Array.from(queryVector)

	// Score each chunk
	const scored = rows.map((row) => {
		const vec = JSON.parse(row.vector) as number[]
		return {
			meetingId: row.meetingId,
			title: row.title,
			startedAt: row.startedAt,
			chunkText: row.chunkText,
			score: cosineSimilarity(qv, vec),
		}
	})

	// Best score per meeting
	const byMeeting = new Map<string, (typeof scored)[0]>()
	for (const item of scored) {
		const prev = byMeeting.get(item.meetingId)
		if (!prev || item.score > prev.score) {
			byMeeting.set(item.meetingId, item)
		}
	}

	return [...byMeeting.values()]
		.sort((a, b) => b.score - a.score)
		.slice(0, limit)
		.map((item) => ({
			meetingId: item.meetingId,
			title: item.title,
			date: formatDate(item.startedAt),
			excerpt: item.chunkText.slice(0, 180) + (item.chunkText.length > 180 ? "…" : ""),
			score: item.score,
		}))
}
