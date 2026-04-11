import { create } from "zustand"
import { updateMeeting } from "@/lib/db/meetings"
import { getAllSettings } from "@/lib/db/settings-repo"
import { createTranscript, insertSegments } from "@/lib/db/transcripts"
import { getTranscriptionProvider } from "@/lib/providers/transcription"
import { nanoid } from "@/lib/utils"
import { useSummaryStore } from "./summary"

export interface TranscriptionJob {
	id: string
	meetingId: string
	audioPath: string
	status: "queued" | "transcribing" | "done" | "failed"
	error?: string
	attempts: number
}

interface TranscriptionStore {
	jobs: TranscriptionJob[]
	enqueue: (job: { meetingId: string; audioPath: string }) => void
	_processNext: () => Promise<void>
}

export const useTranscriptionStore = create<TranscriptionStore>((set, get) => ({
	jobs: [],

	enqueue({ meetingId, audioPath }) {
		const job: TranscriptionJob = {
			id: nanoid(),
			meetingId,
			audioPath,
			status: "queued",
			attempts: 0,
		}
		set((s) => ({ jobs: [...s.jobs, job] }))
		// Kick off processing (will no-op if already running)
		get()._processNext()
	},

	async _processNext() {
		const { jobs } = get()
		const next = jobs.find((j) => j.status === "queued")
		if (!next) return

		// Mark as in-progress
		set((s) => ({
			jobs: s.jobs.map((j) =>
				j.id === next.id ? { ...j, status: "transcribing", attempts: j.attempts + 1 } : j,
			),
		}))

		try {
			const settings = await getAllSettings()
			const provider = getTranscriptionProvider(settings)
			const result = await provider.transcribe(next.audioPath)

			// Persist transcript
			const transcriptId = nanoid()
			await createTranscript({
				id: transcriptId,
				meetingId: next.meetingId,
				rawText: result.text,
				provider: result.provider,
				language: result.language,
				durationMs: result.durationMs,
				createdAt: new Date().toISOString(),
			})

			// Persist segments
			await insertSegments(
				result.segments.map((seg) => ({
					id: nanoid(),
					transcriptId,
					startMs: seg.startMs,
					endMs: seg.endMs,
					text: seg.text,
					speakerLabel: seg.speaker ?? null,
				})),
			)

			// Mark job done
			set((s) => ({
				jobs: s.jobs.map((j) => (j.id === next.id ? { ...j, status: "done" } : j)),
			}))

			// Trigger summarization
			useSummaryStore.getState().enqueue({
				meetingId: next.meetingId,
				transcript: result.text,
			})
		} catch (err) {
			const job = get().jobs.find((j) => j.id === next.id)
			if ((job?.attempts ?? 0) < 2) {
				// Retry: re-queue
				set((s) => ({
					jobs: s.jobs.map((j) => (j.id === next.id ? { ...j, status: "queued" } : j)),
				}))
			} else {
				const message = err instanceof Error ? err.message : String(err)
				set((s) => ({
					jobs: s.jobs.map((j) =>
						j.id === next.id ? { ...j, status: "failed", error: message } : j,
					),
				}))
				await updateMeeting(next.meetingId, { status: "failed" })
			}
		}

		// Process next job in queue
		get()._processNext()
	},
}))
