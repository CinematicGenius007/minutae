import { desc, eq } from "drizzle-orm"
import { getDb } from "./client"
import { meetings, type NewMeeting } from "./schema"

export async function createMeeting(data: NewMeeting) {
	const db = await getDb()
	await db.insert(meetings).values(data)
	return data
}

export async function getMeeting(id: string) {
	const db = await getDb()
	const rows = await db.select().from(meetings).where(eq(meetings.id, id))
	return rows[0] ?? null
}

export async function listMeetings() {
	const db = await getDb()
	return db.select().from(meetings).orderBy(desc(meetings.startedAt))
}

export async function updateMeeting(id: string, data: Partial<Omit<NewMeeting, "id">>) {
	const db = await getDb()
	await db.update(meetings).set(data).where(eq(meetings.id, id))
}

export async function deleteMeeting(id: string) {
	const db = await getDb()
	await db.delete(meetings).where(eq(meetings.id, id))
}
