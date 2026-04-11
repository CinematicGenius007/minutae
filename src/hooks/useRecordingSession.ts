import { invoke } from "@tauri-apps/api/core"
import { useEffect, useRef } from "react"
import { updateMeeting } from "@/lib/db/meetings"
import { useRecordingStore } from "@/store/recording"
import { useTranscriptionStore } from "@/store/transcription"
import { useRecorder } from "./useRecorder"

/**
 * Orchestrates the full recording session lifecycle.
 * Connects useRecorder → Zustand store → Tauri save command → transcription queue.
 */
export function useRecordingSession() {
	const recorder = useRecorder()
	const store = useRecordingStore()
	const enqueueTranscription = useTranscriptionStore((s) => s.enqueue)

	const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

	// Tick duration every second while recording
	useEffect(() => {
		if (store.status === "recording") {
			intervalRef.current = setInterval(store.tickDuration, 1000)
		} else {
			if (intervalRef.current) {
				clearInterval(intervalRef.current)
				intervalRef.current = null
			}
		}
		return () => {
			if (intervalRef.current) clearInterval(intervalRef.current)
		}
	}, [store.status, store.tickDuration])

	async function startRecording() {
		await recorder.start()
		// Meeting row was already created in MeetingPrompt before calling this
	}

	async function pauseRecording() {
		recorder.pause()
		store.pauseRecording()
		if (store.currentMeeting) {
			await updateMeeting(store.currentMeeting.id, { status: "paused" })
		}
	}

	async function resumeRecording() {
		recorder.resume()
		store.resumeRecording()
		if (store.currentMeeting) {
			await updateMeeting(store.currentMeeting.id, { status: "recording" })
		}
	}

	async function stopRecording() {
		const meeting = store.currentMeeting
		if (!meeting) return

		const blob = await recorder.stop()
		store.stopMeeting()

		const durationSeconds = store.durationSeconds
		const dateStr = new Date().toISOString().slice(0, 10)
		const filename = `${dateStr}-${meeting.id}.webm`

		// Save audio file via Tauri
		const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()))
		const filePath = await invoke<string>("save_recording", { bytes, filename })

		// Update meeting row
		await updateMeeting(meeting.id, {
			endedAt: new Date().toISOString(),
			durationSeconds,
			status: "processing",
		})

		// Enqueue transcription
		enqueueTranscription({ meetingId: meeting.id, audioPath: filePath })
	}

	return {
		status: store.status,
		durationSeconds: store.durationSeconds,
		audioLevel: recorder.audioLevel,
		currentMeeting: store.currentMeeting,
		startRecording,
		pauseRecording,
		resumeRecording,
		stopRecording,
	}
}
