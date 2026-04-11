import { eq } from "drizzle-orm"
import { getDb } from "./client"
import { actionItems, type NewActionItem } from "./schema"

export async function createActionItems(data: NewActionItem[]) {
	if (data.length === 0) return
	const db = await getDb()
	await db.insert(actionItems).values(data)
}

export async function getActionItems(meetingId: string) {
	const db = await getDb()
	return db.select().from(actionItems).where(eq(actionItems.meetingId, meetingId))
}

export async function toggleActionItem(id: string, done: boolean) {
	const db = await getDb()
	await db.update(actionItems).set({ done }).where(eq(actionItems.id, id))
}

export async function deleteActionItem(id: string) {
	const db = await getDb()
	await db.delete(actionItems).where(eq(actionItems.id, id))
}
