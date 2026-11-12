import { eq } from "drizzle-orm"
import { getDb } from "./client"
import { type NewSummary, summaries } from "./schema"

export async function createSummary(data: NewSummary) {
	const db = await getDb()
	await db.insert(summaries).values(data)
	return data
}

export async function getSummary(meetingId: string) {
	const db = await getDb()
	const rows = await db.select().from(summaries).where(eq(summaries.meetingId, meetingId))
	return rows[0] ?? null
}
