import { useEffect, useState } from "react"
import { formatDuration } from "@/lib/utils"
import { useRecordingStore } from "@/store/recording"

interface RecordingPillProps {
	durationSeconds: number
	audioLevel: number
	pauseRecording: () => void | Promise<void>
	resumeRecording: () => void | Promise<void>
	stopRecording: () => void | Promise<void>
}

export function RecordingPill({
	durationSeconds,
	audioLevel,
	pauseRecording,
	resumeRecording,
	stopRecording,
}: RecordingPillProps) {
	const status = useRecordingStore((s) => s.status)
	const [displayLevel, setDisplayLevel] = useState(0)

	useEffect(() => {
		const nextLevel = status === "paused" ? 0 : Math.min(1, audioLevel * 4.5)
		setDisplayLevel((current) => current * 0.72 + nextLevel * 0.28)
	}, [audioLevel, status])

	if (status !== "recording" && status !== "paused") return null

	const bars = Array.from({ length: 9 }, (_, index) => {
		const distanceFromCenter = Math.abs(index - 4)
		const baseHeight = Math.max(4, 20 - distanceFromCenter * 3)
		const reactiveBoost = displayLevel * (14 - distanceFromCenter * 1.5)
		const height = Math.round(baseHeight + reactiveBoost)

		return {
			key: index,
			height: status === "paused" ? 4 : height,
			opacity: status === "paused" ? 0.35 : Math.max(0.45, 0.65 + displayLevel * 0.35),
		}
	})

	return (
		<div className="fixed bottom-4 left-1/2 z-50 flex -translate-x-1/2 items-center gap-3 rounded-full border border-border bg-surface px-4 py-2.5 shadow-xl">
			{/* Wave meter */}
			<div className="flex h-8 items-center gap-1 rounded-full bg-bg/55 px-2">
				{bars.map((bar) => (
					<span
						key={bar.key}
						className={`w-1 rounded-full transition-[height,opacity,background-color] duration-150 ${
							status === "paused" ? "bg-text-muted" : "bg-accent"
						}`}
						style={{
							height: `${bar.height}px`,
							opacity: bar.opacity,
						}}
					/>
				))}
			</div>

			{/* Timer */}
			<span className="tabular-nums text-sm font-medium text-text min-w-10.5">
				{formatDuration(durationSeconds)}
			</span>

			{/* Status badge */}
			<span className={`text-xs ${status === "paused" ? "text-text-muted" : "text-accent"}`}>
				{status === "paused" ? "Paused" : "Recording"}
			</span>

			{/* Controls */}
			<div className="flex items-center gap-1">
				{status === "recording" ? (
					<button
						type="button"
						onClick={pauseRecording}
						className="rounded-full p-1.5 hover:bg-surface-raised text-text-muted hover:text-text transition-colors"
						title="Pause"
					>
						<PauseIcon />
					</button>
				) : (
					<button
						type="button"
						onClick={resumeRecording}
						className="rounded-full p-1.5 hover:bg-surface-raised text-text-muted hover:text-text transition-colors"
						title="Resume"
					>
						<PlayIcon />
					</button>
				)}
				<button
					type="button"
					onClick={stopRecording}
					className="rounded-full p-1.5 hover:bg-destructive/10 text-destructive transition-colors"
					title="Stop recording"
				>
					<StopIcon />
				</button>
			</div>
		</div>
	)
}

function PauseIcon() {
	return (
		<svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor" aria-hidden="true">
			<rect x="2" y="1" width="4" height="12" rx="1" />
			<rect x="8" y="1" width="4" height="12" rx="1" />
		</svg>
	)
}

function PlayIcon() {
	return (
		<svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor" aria-hidden="true">
			<path d="M3 2l9 5-9 5V2z" />
		</svg>
	)
}

function StopIcon() {
	return (
		<svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor" aria-hidden="true">
			<rect x="2" y="2" width="10" height="10" rx="1.5" />
		</svg>
	)
}
