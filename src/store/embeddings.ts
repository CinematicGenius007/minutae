import { create } from "zustand"
import { storeEmbeddings } from "@/lib/db/embeddings"
import { chunkText, embed } from "@/lib/embeddings/embed"
import { nanoid } from "@/lib/utils"

export type EmbeddingModelStatus = "idle" | "loading" | "ready" | "error"

export interface EmbeddingJob {
	id: string
	meetingId: string
	transcript: string
	status: "queued" | "embedding" | "done" | "failed"
	error?: string
}

interface EmbeddingsStore {
	jobs: EmbeddingJob[]
	modelStatus: EmbeddingModelStatus
	enqueue: (job: { meetingId: string; transcript: string }) => void
	_processNext: () => Promise<void>
}

export const useEmbeddingsStore = create<EmbeddingsStore>((set, get) => ({
	jobs: [],
	modelStatus: "idle",

	enqueue({ meetingId, transcript }) {
		const job: EmbeddingJob = {
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
			jobs: s.jobs.map((j) => (j.id === next.id ? { ...j, status: "embedding" } : j)),
			modelStatus: s.modelStatus === "idle" ? "loading" : s.modelStatus,
		}))

		try {
			const chunks = chunkText(next.transcript)
			if (chunks.length === 0) {
				set((s) => ({
					jobs: s.jobs.map((j) => (j.id === next.id ? { ...j, status: "done" } : j)),
					modelStatus: "ready",
				}))
				get()._processNext()
				return
			}

			// Embed each chunk (model auto-downloads on first call)
			const embedded: { text: string; index: number; vector: Float32Array }[] = []
			for (let i = 0; i < chunks.length; i++) {
				const vector = await embed(chunks[i])
				embedded.push({ text: chunks[i], index: i, vector })
			}

			set(() => ({ modelStatus: "ready" }))

			await storeEmbeddings(next.meetingId, embedded)

			set((s) => ({
				jobs: s.jobs.map((j) => (j.id === next.id ? { ...j, status: "done" } : j)),
			}))
		} catch (err) {
			const message = err instanceof Error ? err.message : String(err)
			console.error("[embeddings] failed:", message)
			set((s) => ({
				jobs: s.jobs.map((j) =>
					j.id === next.id ? { ...j, status: "failed", error: message } : j,
				),
				modelStatus: "error",
			}))
		}

		get()._processNext()
	},
}))
