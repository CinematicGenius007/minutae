import { Link } from "@tanstack/react-router"
import { useCallback, useEffect, useState } from "react"
import { listMeetings } from "@/lib/db/meetings"
import type { Meeting } from "@/lib/db/schema"
import { formatDate, formatDurationLong } from "@/lib/utils"
import { useRecordingStore } from "@/store/recording"
import { useSummaryStore } from "@/store/summary"
import { useTranscriptionStore } from "@/store/transcription"

const STATUS_BADGE: Record<Meeting["status"], { label: string; className: string }> = {
	recording: {
		label: "Recording",
		className: "bg-[var(--color-accent)]/15 text-[var(--color-accent)]",
	},
	paused: {
		label: "Paused",
		className: "bg-[var(--color-text-muted)]/15 text-[var(--color-text-muted)]",
	},
	processing: { label: "Processing", className: "bg-amber-500/15 text-amber-400" },
	done: { label: "Done", className: "bg-[var(--color-success)]/15 text-[var(--color-success)]" },
	failed: {
		label: "Failed",
		className: "bg-[var(--color-destructive)]/15 text-[var(--color-destructive)]",
	},
}

export function MeetingsPage() {
	const [meetings, setMeetings] = useState<Meeting[]>([])
	const [loading, setLoading] = useState(true)
	const recordingStatus = useRecordingStore((s) => s.status)
	const transcriptionJobs = useTranscriptionStore((s) => s.jobs)
	const summaryJobs = useSummaryStore((s) => s.jobs)
	const jobRefreshKey = `${recordingStatus}:${transcriptionJobs
		.map((job) => `${job.meetingId}:${job.status}`)
		.join("|")}:${summaryJobs.map((job) => `${job.meetingId}:${job.status}`).join("|")}`

	const load = useCallback(async () => {
		const rows = await listMeetings()
		setMeetings(rows)
		setLoading(false)
	}, [])

	useEffect(() => {
		load()
	}, [load])

	// Refresh list when a recording finishes
	useEffect(() => {
		void jobRefreshKey
		load()
	}, [jobRefreshKey, load])

	return (
		<div className="flex flex-col h-full">
			{/* Header */}
			<div className="px-6 pt-5 pb-4 border-b border-border flex items-center justify-between">
				<h1 className="text-base font-semibold text-text">Meetings</h1>
				<span className="text-xs text-text-muted">
					{meetings.length > 0
						? `${meetings.length} meeting${meetings.length === 1 ? "" : "s"}`
						: ""}
				</span>
			</div>

			{/* Content */}
			{loading ? (
				<div className="flex-1 flex items-center justify-center">
					<span className="text-sm text-text-muted">Loading…</span>
				</div>
			) : meetings.length === 0 ? (
				<EmptyState />
			) : (
				<ul className="flex-1 overflow-y-auto divide-y divide-border">
					{meetings.map((m) => (
						<MeetingRow
							key={m.id}
							meeting={m}
							transcriptionJobs={transcriptionJobs}
							summaryJobs={summaryJobs}
						/>
					))}
				</ul>
			)}
		</div>
	)
}

function MeetingRow({
	meeting,
	transcriptionJobs,
	summaryJobs,
}: {
	meeting: Meeting
	transcriptionJobs: { meetingId: string; status: string }[]
	summaryJobs: { meetingId: string; status: string }[]
}) {
	const transcriptionJob = transcriptionJobs.find(
		(job) => job.meetingId === meeting.id && job.status !== "done",
	)
	const summaryJob = summaryJobs.find(
		(job) => job.meetingId === meeting.id && job.status !== "done",
	)

	const badge =
		meeting.status === "processing" && transcriptionJob?.status === "transcribing"
			? { label: "Transcribing", className: "bg-amber-500/15 text-amber-400" }
			: meeting.status === "processing" && summaryJob?.status === "summarizing"
				? { label: "Summarizing", className: "bg-sky-500/15 text-sky-400" }
				: STATUS_BADGE[meeting.status]

	return (
		<li>
			<Link
				to="/meetings/$id"
				params={{ id: meeting.id }}
				className="flex items-center gap-4 px-6 py-3.5 hover:bg-[var(--color-surface)] transition-colors group"
			>
				{/* Colour stripe */}
				<span
					className={`w-1 h-8 rounded-full flex-shrink-0 ${
						meeting.status === "recording"
							? "bg-[var(--color-accent)]"
							: meeting.status === "done"
								? "bg-[var(--color-success)]"
								: meeting.status === "failed"
									? "bg-[var(--color-destructive)]"
									: "bg-[var(--color-border)]"
					}`}
				/>

				{/* Title + date */}
				<div className="flex-1 min-w-0">
					<p className="text-sm font-medium text-[var(--color-text)] truncate">{meeting.title}</p>
					<p className="text-xs text-[var(--color-text-muted)] mt-0.5">
						{formatDate(meeting.startedAt)}
						{meeting.durationSeconds != null && (
							<> · {formatDurationLong(meeting.durationSeconds)}</>
						)}
					</p>
				</div>

				{/* Status badge */}
				<span
					className={`text-[10px] font-medium px-2 py-0.5 rounded-full flex-shrink-0 ${badge.className}`}
				>
					{badge.label}
				</span>

				{/* Chevron */}
				<svg
					aria-hidden="true"
					className="w-3.5 h-3.5 text-border group-hover:text-text-muted transition-colors shrink-0"
					fill="none"
					viewBox="0 0 24 24"
					stroke="currentColor"
					strokeWidth={2}
				>
					<path strokeLinecap="round" strokeLinejoin="round" d="M9 5l7 7-7 7" />
				</svg>
			</Link>
		</li>
	)
}

function EmptyState() {
	return (
		<div className="flex-1 flex flex-col items-center justify-center gap-3 px-8 text-center">
			<div className="w-12 h-12 rounded-xl bg-surface flex items-center justify-center">
				<svg
					aria-hidden="true"
					className="w-6 h-6 text-text-muted"
					fill="none"
					viewBox="0 0 24 24"
					stroke="currentColor"
					strokeWidth={1.5}
				>
					<path
						strokeLinecap="round"
						strokeLinejoin="round"
						d="M12 18.75a6 6 0 006-6v-1.5m-6 7.5a6 6 0 01-6-6v-1.5m6 7.5v3.75m-3.75 0h7.5M12 15.75a3 3 0 01-3-3V4.5a3 3 0 116 0v8.25a3 3 0 01-3 3z"
					/>
				</svg>
			</div>
			<div>
				<p className="text-sm font-medium text-text">No meetings yet</p>
				<p className="text-xs text-text-muted mt-1 max-w-xs">
					Minutae will detect when your mic is active in Zoom, Meet, or Teams and ask if you'd like
					to record.
				</p>
			</div>
		</div>
	)
}
