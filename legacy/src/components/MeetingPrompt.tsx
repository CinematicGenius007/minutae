import { useEffect, useRef, useState } from "react"
import { getAllSettings } from "@/lib/db/settings-repo"
import { nanoid } from "@/lib/utils"
import { useRecordingStore } from "@/store/recording"

const AUTO_DISMISS_MS = 30_000

interface MeetingPromptProps {
	onStartRecording: (meeting: {
		id: string
		title: string
		startedAt: string
		endedAt: null
		durationSeconds: null
		status: "recording"
	}) => Promise<void>
}

export function MeetingPrompt({ onStartRecording }: MeetingPromptProps) {
	const { micActive, promptDismissed, status, dismissPrompt } = useRecordingStore()

	const [visible, setVisible] = useState(false)
	const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

	const shouldShow = micActive && !promptDismissed && status === "detecting"

	useEffect(() => {
		let cancelled = false
		if (shouldShow) {
			// Honour the user's detection settings instead of a hardcoded delay.
			getAllSettings()
				.then((settings) => {
					if (cancelled || !settings.autoDetectMeetings) return
					timerRef.current = setTimeout(
						() => setVisible(true),
						Math.max(1, settings.showPromptDelay) * 1000,
					)
				})
				.catch(() => {})
		} else {
			setVisible(false)
		}
		return () => {
			cancelled = true
			if (timerRef.current) clearTimeout(timerRef.current)
		}
	}, [shouldShow])

	// Auto-dismiss after 30s
	useEffect(() => {
		if (!visible) return
		const t = setTimeout(() => {
			setVisible(false)
			dismissPrompt()
		}, AUTO_DISMISS_MS)
		return () => clearTimeout(t)
	}, [visible, dismissPrompt])

	async function handleStart() {
		const meeting = {
			id: nanoid(),
			title: "Untitled Meeting",
			startedAt: new Date().toISOString(),
			endedAt: null,
			durationSeconds: null,
			status: "recording" as const,
		}
		await onStartRecording(meeting)
		setVisible(false)
	}

	function handleDismiss() {
		setVisible(false)
		dismissPrompt()
	}

	if (!visible) return null

	return (
		<div
			className="fixed top-4 right-4 z-50 w-80 rounded-md border border-border bg-bg p-4 shadow-[var(--shadow-pop)]"
			role="alertdialog"
			aria-label="Meeting detected"
		>
			<div className="flex items-center gap-2 text-xs font-medium text-text-muted">
				<span className="live-dot size-2" />
				Mic is live
			</div>
			<p className="mt-2 text-[15px] font-semibold leading-snug text-text">
				Looks like a meeting. Record it?
			</p>
			<p className="mt-1 text-[13px] leading-relaxed text-text-muted">
				Minutae transcribes on this Mac and writes the notes when you stop.
			</p>
			<div className="mt-4 flex gap-2">
				<button
					type="button"
					onClick={handleStart}
					className="flex-1 rounded-sm bg-accent px-3 py-2 text-[13px] font-medium text-on-accent transition-colors hover:bg-accent-hover"
				>
					Record
				</button>
				<button
					type="button"
					onClick={handleDismiss}
					className="rounded-sm px-3 py-2 text-[13px] font-medium text-text-muted transition-colors hover:bg-surface hover:text-text"
				>
					Not now
				</button>
			</div>
		</div>
	)
}
