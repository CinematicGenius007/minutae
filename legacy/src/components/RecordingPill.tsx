import { useEffect, useState } from "react"
import { PauseIcon, PlayIcon, StopIcon } from "@/components/icons"
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
		<div
			role="status"
			aria-label={status === "paused" ? "Recording paused" : "Recording"}
			className="fixed bottom-5 left-1/2 z-50 flex -translate-x-1/2 items-center gap-3 rounded-full bg-text py-2 pr-2 pl-4 text-bg shadow-[var(--shadow-pop)]"
		>
			<span className={`size-2 rounded-full ${status === "paused" ? "bg-bg/40" : "live-dot"}`} />

			<div className="flex h-6 items-center gap-[3px]">
				{bars.map((bar) => (
					<span
						key={bar.key}
						className="w-[3px] rounded-full bg-bg transition-[height,opacity] duration-150"
						style={{ height: `${Math.round(bar.height * 0.75)}px`, opacity: bar.opacity }}
					/>
				))}
			</div>

			<span className="min-w-11 font-mono text-[13px] tabular-nums">
				{formatDuration(durationSeconds)}
			</span>

			<div className="flex items-center gap-1">
				<button
					type="button"
					onClick={status === "recording" ? pauseRecording : resumeRecording}
					className="grid size-8 place-items-center rounded-full text-bg/80 transition-colors hover:bg-bg/15 hover:text-bg"
					title={status === "recording" ? "Pause" : "Resume"}
				>
					{status === "recording" ? <PauseIcon size={14} /> : <PlayIcon size={14} />}
				</button>
				<button
					type="button"
					onClick={stopRecording}
					className="flex h-8 items-center gap-1.5 rounded-full bg-live px-3 text-[12px] font-medium text-white transition-opacity hover:opacity-90"
					title="Stop and summarize"
				>
					<StopIcon size={12} />
					Stop
				</button>
			</div>
		</div>
	)
}
