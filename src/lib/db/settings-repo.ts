import { eq } from "drizzle-orm"
import type { AppSettings } from "@/lib/settings"
import { getDb } from "./client"
import { settings } from "./schema"

const DEFAULTS: AppSettings = {
	launchAtLogin: false,
	autoDetectMeetings: true,
	showPromptDelay: 5,
	transcriptionProvider: "local",
	whisperModel: "base",
	openaiApiKey: null,
	summaryProvider: "gemini",
	geminiApiKey: null,
	anthropicApiKey: null,
	summaryOpenaiApiKey: null,
	maxRecordingAgeDays: null,
}

export async function getSetting<K extends keyof AppSettings>(key: K): Promise<AppSettings[K]> {
	const db = await getDb()
	const rows = await db.select().from(settings).where(eq(settings.key, key))
	if (!rows[0]) return DEFAULTS[key]
	try {
		return JSON.parse(rows[0].value) as AppSettings[K]
	} catch {
		return DEFAULTS[key]
	}
}

export async function setSetting<K extends keyof AppSettings>(key: K, value: AppSettings[K]) {
	const db = await getDb()
	await db
		.insert(settings)
		.values({
			key,
			value: JSON.stringify(value),
			updatedAt: new Date().toISOString(),
		})
		.onConflictDoUpdate({
			target: settings.key,
			set: {
				value: JSON.stringify(value),
				updatedAt: new Date().toISOString(),
			},
		})
}

export async function getAllSettings(): Promise<AppSettings> {
	const db = await getDb()
	const rows = await db.select().from(settings)
	const result = { ...DEFAULTS }
	for (const row of rows) {
		try {
			;(result as Record<string, unknown>)[row.key] = JSON.parse(row.value)
		} catch {
			// ignore malformed values
		}
	}
	return result
}
