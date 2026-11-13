import { invoke } from "@tauri-apps/api/core"
import { useCallback, useEffect, useState } from "react"
import { convertBlobToWav } from "@/lib/audio/wav"
import { getAllSettings } from "@/lib/db/settings-repo"
import { getTranscriptionProvider } from "@/lib/providers/transcription"
import { nanoid } from "@/lib/utils"
import { useRecordingStore } from "@/store/recording"
import { useRecorder } from "./useRecorder"

export interface DictationEntry {
	id: string
	text: string
	createdAt: string
	durationMs: number
}

type Phase = "idle" | "listening" | "transcribing"

const HISTORY_KEY = "minutae-dictation-history"
const HISTORY_MAX = 50

function loadHistory(): DictationEntry[] {
	try {
		const raw = localStorage.getItem(HISTORY_KEY)
		return raw ? (JSON.parse(raw) as DictationEntry[]) : []
	} catch {
		return []
	}
}

function saveHistory(entries: DictationEntry[]) {
	try {
		localStorage.setItem(HISTORY_KEY, JSON.stringify(entries.slice(0, HISTORY_MAX)))
	} catch {
		// History is a convenience; losing it must never break dictation.
	}
}

/** Whisper marks silence and noise with bracketed tags; they are not speech. */
export function cleanDictation(text: string): string {
	return text
		.replace(/\[(?:BLANK_AUDIO|MUSIC|NOISE|SILENCE)\]/gi, " ")
		.replace(/\((?:music|noise|silence)\)/gi, " ")
		.replace(/\s+/g, " ")
		.trim()
}

/**
 * Voice-to-text: record → on-device whisper → text. Reuses the shared recorder, so it
 * is unavailable while a meeting is being recorded.
 */
export function useDictation() {
	const recorder = useRecorder()
	const meetingStatus = useRecordingStore((s) => s.status)
	const [phase, setPhase] = useState<Phase>("idle")
	const [error, setError] = useState<string | null>(null)
	const [history, setHistory] = useState<DictationEntry[]>(loadHistory)
	const [seconds, setSeconds] = useState(0)

	const meetingBusy = meetingStatus === "recording" || meetingStatus === "paused"

	useEffect(() => {
		if (phase !== "listening") return
		setSeconds(0)
		const t = setInterval(() => setSeconds((s) => s + 1), 1000)
		return () => clearInterval(t)
	}, [phase])

	const start = useCallback(async () => {
		if (meetingBusy || phase !== "idle") return
		setError(null)
		try {
			await recorder.start()
			setPhase("listening")
		} catch (e) {
			setError(e instanceof Error ? e.message : String(e))
		}
	}, [meetingBusy, phase, recorder])

	const stop = useCallback(async () => {
		if (phase !== "listening") return
		setPhase("transcribing")
		const filename = `dictation-${nanoid()}.wav`
		try {
			const blob = await recorder.stop()
			const wav = await convertBlobToWav(blob)
			const path = await invoke<string>("save_recording", { bytes: Array.from(wav), filename })
			try {
				const settings = await getAllSettings()
				const result = await getTranscriptionProvider(settings).transcribe(path)
				const text = cleanDictation(result.text)
				if (!text) {
					setError("Didn’t catch anything. Try again a little closer to the mic.")
				} else {
					const entry: DictationEntry = {
						id: nanoid(),
						text,
						createdAt: new Date().toISOString(),
						durationMs: result.durationMs,
					}
					setHistory((h) => {
						const next = [entry, ...h]
						saveHistory(next)
						return next
					})
				}
			} finally {
				// Dictation clips are scratch audio — never keep them.
				await invoke("delete_recording_file", { filename }).catch(() => {})
			}
		} catch (e) {
			setError(e instanceof Error ? e.message : String(e))
		} finally {
			setPhase("idle")
		}
	}, [phase, recorder])

	const remove = useCallback((id: string) => {
		setHistory((h) => {
			const next = h.filter((e) => e.id !== id)
			saveHistory(next)
			return next
		})
	}, [])

	const clear = useCallback(() => {
		setHistory([])
		saveHistory([])
	}, [])

	return {
		phase,
		error,
		history,
		seconds,
		level: recorder.audioLevel,
		meetingBusy,
		start,
		stop,
		remove,
		clear,
	}
}
