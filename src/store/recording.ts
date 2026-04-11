import { create } from "zustand"
import type { Meeting } from "@/lib/db/schema"

export type RecordingStatus = "idle" | "detecting" | "recording" | "paused" | "processing" | "done"

interface RecordingStore {
	// Mic state
	micActive: boolean
	promptDismissed: boolean
	setMicActive: (active: boolean) => void
	dismissPrompt: () => void
	resetPrompt: () => void

	// Recording state
	status: RecordingStatus
	currentMeeting: Meeting | null
	startedAt: Date | null
	durationSeconds: number

	// Actions
	startMeeting: (meeting: Meeting) => void
	pauseRecording: () => void
	resumeRecording: () => void
	stopMeeting: () => void
	completeMeeting: () => void
	failMeeting: () => void
	setStatus: (status: RecordingStatus) => void
	tickDuration: () => void
}

export const useRecordingStore = create<RecordingStore>((set, get) => ({
	// Mic
	micActive: false,
	promptDismissed: false,

	setMicActive: (active) =>
		set((s) => ({
			micActive: active,
			// Reset dismissal when mic goes idle so prompt can show again next time
			promptDismissed: active ? s.promptDismissed : false,
			status:
				active && s.status === "idle"
					? "detecting"
					: active
						? s.status
						: s.status === "detecting"
							? "idle"
							: s.status,
		})),

	dismissPrompt: () => set({ promptDismissed: true }),

	resetPrompt: () => set({ promptDismissed: false }),

	// Recording
	status: "idle",
	currentMeeting: null,
	startedAt: null,
	durationSeconds: 0,

	startMeeting: (meeting) =>
		set({
			currentMeeting: meeting,
			status: "recording",
			startedAt: new Date(),
			durationSeconds: 0,
			promptDismissed: true,
		}),

	pauseRecording: () => set({ status: "paused" }),

	resumeRecording: () => set({ status: "recording" }),

	stopMeeting: () =>
		set({
			status: "processing",
			micActive: false,
		}),

	completeMeeting: () =>
		set({
			status: "done",
			currentMeeting: null,
			startedAt: null,
			durationSeconds: 0,
			micActive: false,
		}),

	failMeeting: () =>
		set({
			status: "idle",
			currentMeeting: null,
			startedAt: null,
			durationSeconds: 0,
			micActive: false,
		}),

	setStatus: (status) => set({ status }),

	tickDuration: () => {
		if (get().status === "recording") {
			set((s) => ({ durationSeconds: s.durationSeconds + 1 }))
		}
	},
}))
