import { create } from "zustand"
import { createActionItems } from "@/lib/db/action-items"
import { getMeeting, updateMeeting } from "@/lib/db/meetings"
import { getAllSettings } from "@/lib/db/settings-repo"
import { createSummary } from "@/lib/db/summaries"
import { writeSummaryFile } from "@/lib/markdown"
import { getSummaryProvider } from "@/lib/providers/summary"
import { nanoid } from "@/lib/utils"
import { useEmbeddingsStore } from "@/store/embeddings"

export interface SummaryJob {
	id: string
	meetingId: string
	transcript: string
	status: "queued" | "summarizing" | "done" | "failed"
	error?: string
}

interface SummaryStore {
	jobs: SummaryJob[]
	enqueue: (job: { meetingId: string; transcript: string }) => void
	_processNext: () => Promise<void>
}

export const useSummaryStore = create<SummaryStore>((set, get) => ({
	jobs: [],

	enqueue({ meetingId, transcript }) {
		const job: SummaryJob = {
			id: nanoid(),
			meetingId,
			transcript,
			status: "queued",
		}
		set((s) => ({ jobs: [...s.jobs, job] }))
		get()._processNext()
	},

	async _processNext() {
		const { jobs } = get()
		const next = jobs.find((j) => j.status === "queued")
		if (!next) return

		set((s) => ({
			jobs: s.jobs.map((j) => (j.id === next.id ? { ...j, status: "summarizing" } : j)),
		}))

		try {
			const settings = await getAllSettings()
			const provider = getSummaryProvider(settings)

			if (!provider) {
				// No provider configured — mark done without summary
				await updateMeeting(next.meetingId, { status: "done" })
				set((s) => ({
					jobs: s.jobs.map((j) => (j.id === next.id ? { ...j, status: "done" } : j)),
				}))
				get()._processNext()
				return
			}

			const result = await provider.summarize(next.transcript, next.meetingId)

			// Persist summary
			await createSummary({
				id: nanoid(),
				meetingId: next.meetingId,
				markdownContent: next.transcript, // raw transcript stored separately
				model: provider.name,
				provider: provider.name,
				createdAt: new Date().toISOString(),
			})

			// Persist action items
			await createActionItems(
				result.actionItems.map((text) => ({
					id: nanoid(),
					meetingId: next.meetingId,
					text,
					done: false,
					createdAt: new Date().toISOString(),
				})),
			)

			// Update meeting title + status
			const recordingFilename = `${next.meetingId}.webm`
			const meeting = await getMeeting(next.meetingId)
			if (meeting) {
				await updateMeeting(next.meetingId, {
					title: result.title,
					status: "done",
				})
				await writeSummaryFile({ ...meeting, title: result.title }, result, recordingFilename)
			}

			set((s) => ({
				jobs: s.jobs.map((j) => (j.id === next.id ? { ...j, status: "done" } : j)),
			}))

			// Kick off background embedding (non-blocking; fails silently)
			useEmbeddingsStore.getState().enqueue({
				meetingId: next.meetingId,
				transcript: next.transcript,
			})
		} catch (err) {
			const message = err instanceof Error ? err.message : String(err)
			set((s) => ({
				jobs: s.jobs.map((j) =>
					j.id === next.id ? { ...j, status: "failed", error: message } : j,
				),
			}))
			await updateMeeting(next.meetingId, { status: "failed" }).catch(() => {})
		}

		get()._processNext()
	},
}))
