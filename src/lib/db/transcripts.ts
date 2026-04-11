import { eq } from "drizzle-orm"
import { getDb } from "./client"
import { type NewSegment, type NewTranscript, segments, transcripts } from "./schema"

export async function createTranscript(data: NewTranscript) {
	const db = await getDb()
	await db.insert(transcripts).values(data)
	return data
}

export async function getTranscript(meetingId: string) {
	const db = await getDb()
	const rows = await db.select().from(transcripts).where(eq(transcripts.meetingId, meetingId))
	return rows[0] ?? null
}

export async function insertSegments(data: NewSegment[]) {
	if (data.length === 0) return
	const db = await getDb()
	// Insert in chunks of 100 to avoid SQLite variable limits
	for (let i = 0; i < data.length; i += 100) {
		await db.insert(segments).values(data.slice(i, i + 100))
	}
}

export async function getSegments(transcriptId: string) {
	const db = await getDb()
	return db.select().from(segments).where(eq(segments.transcriptId, transcriptId))
}
