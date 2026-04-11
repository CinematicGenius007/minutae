export interface AppSettings {
	// General
	launchAtLogin: boolean
	autoDetectMeetings: boolean
	showPromptDelay: number // seconds before showing the "start recording?" prompt

	// Transcription
	transcriptionProvider: "local" | "whisper-api"
	whisperModel: "tiny" | "base" | "small" | "medium"
	openaiApiKey: string | null // used for Whisper API

	// Summary
	summaryProvider: "gemini" | "claude" | "openai"
	geminiApiKey: string | null
	anthropicApiKey: string | null
	summaryOpenaiApiKey: string | null

	// Storage
	maxRecordingAgeDays: number | null // null = keep forever
}
