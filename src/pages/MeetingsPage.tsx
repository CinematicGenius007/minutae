import { Link } from "@tanstack/react-router"
import { invoke } from "@tauri-apps/api/core"
import { useCallback, useEffect, useMemo, useState } from "react"
import { CheckIcon, SearchIcon } from "@/components/icons"
import { listMeetings } from "@/lib/db/meetings"
import type { Meeting } from "@/lib/db/schema"
import { getAllSettings } from "@/lib/db/settings-repo"
import type { AppSettings } from "@/lib/settings"
import { formatDurationLong } from "@/lib/utils"
import { useRecordingStore } from "@/store/recording"
import { useSummaryStore } from "@/store/summary"
import { useTranscriptionStore } from "@/store/transcription"

type Chip = { label: string; tone: "live" | "work" | "bad" | "muted" } | null

export function MeetingsPage() {
	const [meetings, setMeetings] = useState<Meeting[]>([])
	const [loading, setLoading] = useState(true)
	const [filter, setFilter] = useState("")
	const recordingStatus = useRecordingStore((s) => s.status)
	const transcriptionJobs = useTranscriptionStore((s) => s.jobs)
	const summaryJobs = useSummaryStore((s) => s.jobs)
	const jobRefreshKey = `${recordingStatus}:${transcriptionJobs
		.map((job) => `${job.meetingId}:${job.status}`)
		.join("|")}:${summaryJobs.map((job) => `${job.meetingId}:${job.status}`).join("|")}`

	const load = useCallback(async () => {
		setMeetings(await listMeetings())
		setLoading(false)
	}, [])

	// biome-ignore lint/correctness/useExhaustiveDependencies: jobRefreshKey is the refresh trigger
	useEffect(() => {
		load()
	}, [load, jobRefreshKey])

	const visible = useMemo(() => {
		const q = filter.trim().toLowerCase()
		return q ? meetings.filter((m) => m.title.toLowerCase().includes(q)) : meetings
	}, [meetings, filter])

	const groups = useMemo(() => groupByDay(visible), [visible])

	return (
		<div className="mx-auto flex min-h-full max-w-3xl flex-col px-8 pt-14 pb-24">
			<header className="flex items-end justify-between gap-4">
				<div>
					<h1 className="text-[28px] font-semibold tracking-tight text-text">Meetings</h1>
					<p className="mt-1 text-[13px] text-text-muted">
						{loading
							? " "
							: meetings.length === 0
								? "Nothing recorded yet."
								: `${meetings.length} recorded · everything stays on this Mac`}
					</p>
				</div>
				{meetings.length > 4 && (
					<label className="flex w-56 items-center gap-2 rounded-sm border border-border px-3 py-1.5 text-[13px] text-text-muted focus-within:border-accent">
						<SearchIcon size={14} />
						<input
							value={filter}
							onChange={(e) => setFilter(e.target.value)}
							placeholder="Filter by title"
							className="min-w-0 flex-1 bg-transparent text-text outline-none placeholder:text-text-muted"
						/>
					</label>
				)}
			</header>

			<SetupChecklist />

			{loading ? null : meetings.length === 0 ? (
				<EmptyState />
			) : visible.length === 0 ? (
				<p className="mt-16 text-center text-[13px] text-text-muted">
					No meetings match “{filter}”.
				</p>
			) : (
				<div className="mt-8 space-y-8">
					{groups.map((group) => (
						<section key={group.label}>
							<h2 className="mb-1 text-[11px] font-semibold tracking-[0.08em] text-text-muted uppercase">
								{group.label}
							</h2>
							<ul className="divide-y divide-border border-y border-border">
								{group.items.map((m) => (
									<MeetingRow
										key={m.id}
										meeting={m}
										transcriptionJobs={transcriptionJobs}
										summaryJobs={summaryJobs}
									/>
								))}
							</ul>
						</section>
					))}
				</div>
			)}
		</div>
	)
}

function groupByDay(meetings: Meeting[]) {
	const today = startOfDay(new Date()).getTime()
	const day = 86_400_000
	const order: string[] = []
	const map = new Map<string, Meeting[]>()

	for (const m of meetings) {
		const t = startOfDay(new Date(m.startedAt)).getTime()
		const label =
			t >= today
				? "Today"
				: t >= today - day
					? "Yesterday"
					: new Date(m.startedAt).toLocaleDateString("en-US", {
							weekday: "long",
							month: "short",
							day: "numeric",
						})
		if (!map.has(label)) {
			map.set(label, [])
			order.push(label)
		}
		map.get(label)?.push(m)
	}

	return order.map((label) => ({ label, items: map.get(label) ?? [] }))
}

function startOfDay(d: Date) {
	return new Date(d.getFullYear(), d.getMonth(), d.getDate())
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
	const transcribing = transcriptionJobs.some(
		(j) => j.meetingId === meeting.id && j.status === "transcribing",
	)
	const summarizing = summaryJobs.some(
		(j) => j.meetingId === meeting.id && j.status === "summarizing",
	)

	const chip: Chip =
		meeting.status === "recording"
			? { label: "Recording", tone: "live" }
			: meeting.status === "paused"
				? { label: "Paused", tone: "muted" }
				: meeting.status === "failed"
					? { label: "Needs attention", tone: "bad" }
					: meeting.status === "processing"
						? {
								label: transcribing ? "Transcribing" : summarizing ? "Summarizing" : "Queued",
								tone: "work",
							}
						: null

	const time = new Date(meeting.startedAt).toLocaleTimeString("en-US", {
		hour: "numeric",
		minute: "2-digit",
	})

	return (
		<li>
			<Link
				to="/meetings/$id"
				params={{ id: meeting.id }}
				className="group -mx-3 flex items-center gap-4 rounded-sm px-3 py-3.5 transition-colors hover:bg-surface"
			>
				<span className="w-[68px] shrink-0 font-mono text-xs text-text-muted tabular-nums">
					{time}
				</span>
				<span className="min-w-0 flex-1 truncate text-[15px] font-medium text-text">
					{meeting.title}
				</span>
				{chip && <StatusChip chip={chip} />}
				<span className="w-14 shrink-0 text-right font-mono text-xs text-text-muted tabular-nums">
					{meeting.durationSeconds != null ? formatDurationLong(meeting.durationSeconds) : ""}
				</span>
			</Link>
		</li>
	)
}

function StatusChip({ chip }: { chip: NonNullable<Chip> }) {
	const tone = {
		live: "text-live",
		work: "text-accent",
		bad: "text-destructive",
		muted: "text-text-muted",
	}[chip.tone]

	return (
		<span className={`flex shrink-0 items-center gap-1.5 text-xs font-medium ${tone}`}>
			<span
				className={`size-1.5 rounded-full bg-current ${chip.tone === "live" ? "live-dot" : chip.tone === "work" ? "animate-pulse" : ""}`}
			/>
			{chip.label}
		</span>
	)
}

// ─── First-run setup ─────────────────────────────────────────────────────────

interface LocalStatus {
	binary_exists: boolean
	model_exists: boolean
}

function hasSummaryKey(s: AppSettings) {
	switch (s.summaryProvider) {
		case "gemini":
			return Boolean(s.geminiApiKey)
		case "claude":
			return Boolean(s.anthropicApiKey)
		case "openai":
			return Boolean(s.summaryOpenaiApiKey)
		case "openrouter":
			return Boolean(s.openRouterApiKey)
	}
}

function SetupChecklist() {
	const [steps, setSteps] = useState<{ label: string; hint: string; done: boolean }[] | null>(null)

	useEffect(() => {
		let cancelled = false
		async function check() {
			const settings = await getAllSettings()
			let transcriptionReady =
				settings.transcriptionProvider === "whisper-api" && Boolean(settings.openaiApiKey)
			if (settings.transcriptionProvider === "local") {
				const status = await invoke<LocalStatus>("get_local_transcription_status", {
					model: settings.whisperModel,
				}).catch(() => null)
				transcriptionReady = Boolean(status?.binary_exists && status?.model_exists)
			}
			if (cancelled) return
			setSteps([
				{
					label: "Transcription engine",
					hint: "Download a Whisper model so audio never leaves your Mac.",
					done: transcriptionReady,
				},
				{
					label: "Summary provider",
					hint: "Add one API key to get notes and action items.",
					done: hasSummaryKey(settings),
				},
			])
		}
		check().catch(() => {})
		return () => {
			cancelled = true
		}
	}, [])

	if (!steps || steps.every((s) => s.done)) return null

	return (
		<section className="mt-8 rounded-md border border-border bg-surface p-5">
			<div className="flex items-baseline justify-between">
				<h2 className="text-[15px] font-semibold text-text">Finish setup</h2>
				<Link to="/settings" className="text-[13px] font-medium text-accent hover:underline">
					Open settings
				</Link>
			</div>
			<ul className="mt-3 space-y-2.5">
				{steps.map((step) => (
					<li key={step.label} className="flex items-start gap-3">
						<span
							className={`mt-0.5 grid size-5 shrink-0 place-items-center rounded-full border ${step.done ? "border-success bg-success text-on-accent" : "border-border bg-bg"}`}
						>
							{step.done && <CheckIcon size={12} strokeWidth={3} />}
						</span>
						<div>
							<p
								className={`text-[13px] font-medium ${step.done ? "text-text-muted line-through" : "text-text"}`}
							>
								{step.label}
							</p>
							{!step.done && <p className="text-[13px] text-text-muted">{step.hint}</p>}
						</div>
					</li>
				))}
			</ul>
		</section>
	)
}

function EmptyState() {
	return (
		<div className="mt-20 text-center">
			<p className="text-[22px] font-semibold tracking-tight text-text">
				Open a call. We’ll notice.
			</p>
			<p className="mx-auto mt-2 max-w-sm text-[14px] leading-relaxed text-text-muted">
				When your mic goes live in Zoom, Meet or Teams, Minutae offers to record. Or press{" "}
				<kbd className="rounded border border-border px-1.5 py-0.5 font-sans text-xs">⌘⇧R</kbd> to
				start any time.
			</p>
		</div>
	)
}
