import { invoke } from "@tauri-apps/api/core"
import { useEffect, useRef } from "react"
import { toast } from "sonner"
import { convertBlobToWav } from "@/lib/audio/wav"
import { createMeeting, updateMeeting } from "@/lib/db/meetings"
import { createRecording } from "@/lib/db/recordings"
import { nanoid } from "@/lib/utils"
import { useRecordingStore } from "@/store/recording"
import { useTranscriptionStore } from "@/store/transcription"
import { useRecorder } from "./useRecorder"

type SessionMeeting = {
	id: string
	title: string
	startedAt: string
	endedAt: null
	durationSeconds: null
	status: "recording"
}

/**
 * Orchestrates the full recording session lifecycle.
 * Connects useRecorder → Zustand store → Tauri save command → transcription queue.
 *
 * Two entry points:
 *  - startRecording()      called by MeetingPrompt after it creates the meeting row
 *  - startManualSession()  creates the meeting row itself, then starts recording
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

	async function beginRecording(meeting: SessionMeeting) {
		try {
			await recorder.start()
			store.startMeeting(meeting)
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error)
			await updateMeeting(meeting.id, { status: "failed" }).catch(() => {})
			store.failMeeting()
			console.error("[recording-session] start failed", {
				meetingId: meeting.id,
				storeStatus: store.status,
				debug: recorder.debugState(),
				error: message,
			})
			toast.error("Couldn't start recording", {
				description: message,
				duration: 6000,
			})
		}
	}

	/** Called from MeetingPrompt. */
	async function startRecording(meeting: SessionMeeting) {
		await createMeeting(meeting)
		await beginRecording(meeting)
	}

	/**
	 * Called from the manual "New Recording" button or Cmd+Shift+R.
	 * Creates the meeting row and starts recording immediately — no prompt needed.
	 */
	async function startManualSession() {
		if (store.status !== "idle" && store.status !== "detecting") return

		const meeting = {
			id: nanoid(),
			title: "Untitled Meeting",
			startedAt: new Date().toISOString(),
			endedAt: null,
			durationSeconds: null,
			status: "recording" as const,
		}

		await createMeeting(meeting)
		await beginRecording(meeting)
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

		let blob: Blob
		try {
			blob = await recorder.stop()
		} catch (error) {
			const debug = recorder.debugState()
			const message = error instanceof Error ? error.message : String(error)
			console.error("[recording-session] stop failed", {
				meetingId: meeting.id,
				storeStatus: store.status,
				debug,
				error: message,
			})
			toast.error("Couldn't stop recording", {
				description: `${message} (${debug.mediaRecorderState ?? "no-recorder"})`,
				duration: 6000,
			})
			return
		}
		store.stopMeeting()

		const durationSeconds = store.durationSeconds
		const dateStr = new Date().toISOString().slice(0, 10)
		const wavBytes = await convertBlobToWav(blob)
		const filename = `${dateStr}-${meeting.id}.wav`

		// Save audio file via Tauri
		const filePath = await invoke<string>("save_recording", {
			bytes: Array.from(wavBytes),
			filename,
		})
		await createRecording({
			id: nanoid(),
			meetingId: meeting.id,
			filePath,
			fileSizeBytes: wavBytes.byteLength,
			createdAt: new Date().toISOString(),
		})

		// Update meeting row
		await updateMeeting(meeting.id, {
			endedAt: new Date().toISOString(),
			durationSeconds,
			status: "processing",
		})

		// Enqueue transcription
		enqueueTranscription({ meetingId: meeting.id, audioPath: filePath })
	}

	const isIdle = store.status === "idle" || store.status === "detecting"

	return {
		status: store.status,
		durationSeconds: store.durationSeconds,
		audioLevel: recorder.audioLevel,
		currentMeeting: store.currentMeeting,
		canStartManual: isIdle,
		startRecording,
		startManualSession,
		pauseRecording,
		resumeRecording,
		stopRecording,
	}
}
