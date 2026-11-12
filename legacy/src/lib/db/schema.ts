import { sql } from "drizzle-orm"
import { integer, real, sqliteTable, text } from "drizzle-orm/sqlite-core"

// ─── meetings ────────────────────────────────────────────────────────────────

export const meetings = sqliteTable("meetings", {
	id: text("id").primaryKey().notNull(),
	title: text("title").notNull().default("Untitled Meeting"),
	startedAt: text("started_at").notNull(),
	endedAt: text("ended_at"),
	durationSeconds: integer("duration_seconds"),
	status: text("status", {
		enum: ["recording", "paused", "processing", "done", "failed"],
	})
		.notNull()
		.default("recording"),
})

export type Meeting = typeof meetings.$inferSelect
export type NewMeeting = typeof meetings.$inferInsert

// ─── recordings ──────────────────────────────────────────────────────────────

export const recordings = sqliteTable("recordings", {
	id: text("id").primaryKey().notNull(),
	meetingId: text("meeting_id")
		.notNull()
		.references(() => meetings.id, { onDelete: "cascade" }),
	filePath: text("file_path").notNull(),
	fileSizeBytes: integer("file_size_bytes"),
	createdAt: text("created_at").notNull(),
})

export type Recording = typeof recordings.$inferSelect
export type NewRecording = typeof recordings.$inferInsert

// ─── transcripts ─────────────────────────────────────────────────────────────

export const transcripts = sqliteTable("transcripts", {
	id: text("id").primaryKey().notNull(),
	meetingId: text("meeting_id")
		.notNull()
		.references(() => meetings.id, { onDelete: "cascade" }),
	rawText: text("raw_text").notNull(),
	provider: text("provider", { enum: ["whisper-local", "whisper-api"] })
		.notNull()
		.default("whisper-local"),
	language: text("language"),
	durationMs: integer("duration_ms"),
	createdAt: text("created_at").notNull(),
})

export type Transcript = typeof transcripts.$inferSelect
export type NewTranscript = typeof transcripts.$inferInsert

// ─── segments ────────────────────────────────────────────────────────────────

export const segments = sqliteTable("segments", {
	id: text("id").primaryKey().notNull(),
	transcriptId: text("transcript_id")
		.notNull()
		.references(() => transcripts.id, { onDelete: "cascade" }),
	startMs: integer("start_ms").notNull(),
	endMs: integer("end_ms").notNull(),
	text: text("text").notNull(),
	speakerLabel: text("speaker_label"),
})

export type Segment = typeof segments.$inferSelect
export type NewSegment = typeof segments.$inferInsert

// ─── summaries ───────────────────────────────────────────────────────────────

export const summaries = sqliteTable("summaries", {
	id: text("id").primaryKey().notNull(),
	meetingId: text("meeting_id")
		.notNull()
		.references(() => meetings.id, { onDelete: "cascade" }),
	markdownContent: text("markdown_content").notNull(),
	model: text("model").notNull(),
	provider: text("provider").notNull(),
	createdAt: text("created_at").notNull(),
})

export type Summary = typeof summaries.$inferSelect
export type NewSummary = typeof summaries.$inferInsert

// ─── action_items ────────────────────────────────────────────────────────────

export const actionItems = sqliteTable("action_items", {
	id: text("id").primaryKey().notNull(),
	meetingId: text("meeting_id")
		.notNull()
		.references(() => meetings.id, { onDelete: "cascade" }),
	text: text("text").notNull(),
	done: integer("done", { mode: "boolean" }).notNull().default(false),
	createdAt: text("created_at").notNull(),
})

export type ActionItem = typeof actionItems.$inferSelect
export type NewActionItem = typeof actionItems.$inferInsert

// ─── embeddings ──────────────────────────────────────────────────────────────

export const embeddings = sqliteTable("embeddings", {
	id: text("id").primaryKey().notNull(),
	meetingId: text("meeting_id")
		.notNull()
		.references(() => meetings.id, { onDelete: "cascade" }),
	chunkText: text("chunk_text").notNull(),
	chunkIndex: integer("chunk_index").notNull(),
	vector: text("vector").notNull(), // JSON-encoded Float32Array
})

export type Embedding = typeof embeddings.$inferSelect
export type NewEmbedding = typeof embeddings.$inferInsert

// ─── api_usage_log ───────────────────────────────────────────────────────────

export const apiUsageLog = sqliteTable("api_usage_log", {
	id: text("id").primaryKey().notNull(),
	meetingId: text("meeting_id").references(() => meetings.id, {
		onDelete: "set null",
	}),
	provider: text("provider").notNull(),
	model: text("model").notNull(),
	endpoint: text("endpoint").notNull(),
	inputTokens: integer("input_tokens").notNull().default(0),
	outputTokens: integer("output_tokens").notNull().default(0),
	costUsd: real("cost_usd").notNull().default(0),
	createdAt: text("created_at").notNull(),
})

export type ApiUsageLog = typeof apiUsageLog.$inferSelect
export type NewApiUsageLog = typeof apiUsageLog.$inferInsert

// ─── settings ────────────────────────────────────────────────────────────────

export const settings = sqliteTable("settings", {
	key: text("key").primaryKey().notNull(),
	value: text("value").notNull(),
	updatedAt: text("updated_at").notNull().default(sql`(datetime('now'))`),
})

export type Setting = typeof settings.$inferSelect
