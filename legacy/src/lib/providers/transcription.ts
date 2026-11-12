import { invoke } from "@tauri-apps/api/core"
import type { AppSettings } from "@/lib/settings"

export interface TranscriptionSegment {
	startMs: number
	endMs: number
	text: string
	speaker?: string
}

export interface TranscriptionResult {
	text: string
	segments: TranscriptionSegment[]
	language: string
	durationMs: number
	provider: "whisper-local" | "whisper-api"
}

export interface TranscriptionProvider {
	name: string
	transcribe(audioPath: string): Promise<TranscriptionResult>
}

// ─── Local whisper.cpp provider ──────────────────────────────────────────────

export class WhisperLocalProvider implements TranscriptionProvider {
	name = "whisper-local"

	constructor(private model: AppSettings["whisperModel"]) {}

	async transcribe(audioPath: string): Promise<TranscriptionResult> {
		const raw = await invoke<{
			text: string
			language: string
			duration_ms: number
			segments: { start_ms: number; end_ms: number; text: string }[]
		}>("transcribe_local", { audioPath, model: this.model })

		return {
			text: raw.text,
			language: raw.language,
			durationMs: raw.duration_ms,
			provider: "whisper-local",
			segments: raw.segments.map((s) => ({
				startMs: s.start_ms,
				endMs: s.end_ms,
				text: s.text,
			})),
		}
	}
}

// ─── Cloud Whisper API provider ──────────────────────────────────────────────

export class WhisperAPIProvider implements TranscriptionProvider {
	name = "whisper-api"

	constructor(private apiKey: string) {}

	async transcribe(audioPath: string): Promise<TranscriptionResult> {
		// Read the file via Tauri and POST to OpenAI
		const { readFile } = await import("@tauri-apps/plugin-fs")
		const bytes = await readFile(audioPath)
		const blob = new Blob([bytes], { type: "audio/webm" })

		const form = new FormData()
		form.append("file", blob, "audio.webm")
		form.append("model", "whisper-1")
		form.append("response_format", "verbose_json")

		const res = await fetch("https://api.openai.com/v1/audio/transcriptions", {
			method: "POST",
			headers: { Authorization: `Bearer ${this.apiKey}` },
			body: form,
		})

		if (!res.ok) {
			const err = await res.text()
			throw new Error(`Whisper API error ${res.status}: ${err}`)
		}

		const data = (await res.json()) as {
			text: string
			language: string
			duration: number
			segments: { start: number; end: number; text: string }[]
		}

		return {
			text: data.text,
			language: data.language,
			durationMs: Math.round(data.duration * 1000),
			provider: "whisper-api",
			segments: data.segments.map((s) => ({
				startMs: Math.round(s.start * 1000),
				endMs: Math.round(s.end * 1000),
				text: s.text.trim(),
			})),
		}
	}
}

// ─── Factory ─────────────────────────────────────────────────────────────────

export function getTranscriptionProvider(
	settings: Pick<AppSettings, "transcriptionProvider" | "whisperModel" | "openaiApiKey">,
): TranscriptionProvider {
	if (settings.transcriptionProvider === "whisper-api" && settings.openaiApiKey) {
		return new WhisperAPIProvider(settings.openaiApiKey)
	}
	return new WhisperLocalProvider(settings.whisperModel)
}
