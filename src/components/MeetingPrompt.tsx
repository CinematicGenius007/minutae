import { useEffect, useRef, useState } from "react"
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
		if (shouldShow) {
			// Brief delay so prompt doesn't flash on very short mic blips
			timerRef.current = setTimeout(() => setVisible(true), 2000)
		} else {
			setVisible(false)
			if (timerRef.current) clearTimeout(timerRef.current)
		}
		return () => {
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
			className="fixed top-4 right-4 z-50 w-72 rounded-md border border-border bg-surface p-4 shadow-xl"
			role="alertdialog"
			aria-label="Meeting detected"
		>
			<div className="flex items-start gap-3">
				{/* Pulsing mic indicator */}
				<span className="mt-0.5 flex h-3 w-3 shrink-0">
					<span className="absolute inline-flex h-3 w-3 animate-ping rounded-full bg-accent opacity-75" />
					<span className="relative inline-flex h-3 w-3 rounded-full bg-accent" />
				</span>

				<div className="flex-1 min-w-0">
					<p className="text-sm font-medium text-text">Looks like you're in a meeting</p>
					<p className="mt-0.5 text-xs text-text-muted">
						Start recording to get a summary and action items.
					</p>

					<div className="mt-3 flex gap-2">
						<button
							type="button"
							onClick={handleStart}
							className="flex-1 rounded-sm bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent-hover transition-colors"
						>
							Start Recording
						</button>
						<button
							type="button"
							onClick={handleDismiss}
							className="rounded-sm px-3 py-1.5 text-xs text-text-muted hover:text-text hover:bg-surface-raised transition-colors"
						>
							Dismiss
						</button>
					</div>
				</div>
			</div>
		</div>
	)
}
