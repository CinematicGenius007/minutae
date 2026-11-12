import { eq } from "drizzle-orm"
import { getDb } from "./client"
import { type NewRecording, recordings } from "./schema"

export async function createRecording(data: NewRecording) {
	const db = await getDb()
	await db.insert(recordings).values(data)
	return data
}

export async function getRecording(meetingId: string) {
	const db = await getDb()
	const rows = await db.select().from(recordings).where(eq(recordings.meetingId, meetingId))
	return rows[0] ?? null
}
