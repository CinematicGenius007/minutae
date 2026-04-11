import { useRecordingSession } from "@/hooks/useRecordingSession"
import { formatDuration } from "@/lib/utils"
import { useRecordingStore } from "@/store/recording"

export function RecordingPill() {
	const status = useRecordingStore((s) => s.status)
	const { durationSeconds, audioLevel, pauseRecording, resumeRecording, stopRecording } =
		useRecordingSession()

	if (status !== "recording" && status !== "paused") return null

	// VU meter: 8 bars
	const bars = Array.from({ length: 8 }, (_, i) => {
		const threshold = (i + 1) / 8
		return audioLevel >= threshold
	})

	return (
		<div className="fixed bottom-4 left-1/2 -translate-x-1/2 z-50 flex items-center gap-3 rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-2.5 shadow-xl">
			{/* VU meter */}
			<div className="flex items-end gap-0.5 h-4">
				{bars.map((active, i) => (
					<span
						// biome-ignore lint/suspicious/noArrayIndexKey: stable visual bars
						key={i}
						className={`w-1 rounded-sm transition-all duration-75 ${
							active
								? status === "recording"
									? "bg-[var(--color-accent)] h-4"
									: "bg-[var(--color-text-muted)] h-2"
								: "bg-[var(--color-border)] h-1"
						}`}
					/>
				))}
			</div>

			{/* Timer */}
			<span className="tabular-nums text-sm font-medium text-[var(--color-text)] min-w-[42px]">
				{formatDuration(durationSeconds)}
			</span>

			{/* Status badge */}
			<span className="text-xs text-[var(--color-text-muted)]">
				{status === "paused" ? "Paused" : "Recording"}
			</span>

			{/* Controls */}
			<div className="flex items-center gap-1">
				{status === "recording" ? (
					<button
						type="button"
						onClick={pauseRecording}
						className="rounded-full p-1.5 hover:bg-[var(--color-surface-raised)] text-[var(--color-text-muted)] hover:text-[var(--color-text)] transition-colors"
						title="Pause"
					>
						<PauseIcon />
					</button>
				) : (
					<button
						type="button"
						onClick={resumeRecording}
						className="rounded-full p-1.5 hover:bg-[var(--color-surface-raised)] text-[var(--color-text-muted)] hover:text-[var(--color-text)] transition-colors"
						title="Resume"
					>
						<PlayIcon />
					</button>
				)}
				<button
					type="button"
					onClick={stopRecording}
					className="rounded-full p-1.5 hover:bg-[var(--color-destructive)]/10 text-[var(--color-destructive)] transition-colors"
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
