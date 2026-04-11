import { Link, useNavigate, useParams } from "@tanstack/react-router"
import { useCallback, useEffect, useRef, useState } from "react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import { toast } from "sonner"
import { getActionItems, toggleActionItem } from "@/lib/db/action-items"
import { deleteMeetingAndAssets, getMeeting, updateMeeting } from "@/lib/db/meetings"
import { getRecording } from "@/lib/db/recordings"
import type { ActionItem, Meeting, Segment, Summary, Transcript } from "@/lib/db/schema"
import { getSummary } from "@/lib/db/summaries"
import { getSegments, getTranscript } from "@/lib/db/transcripts"
import { formatDate, formatDurationLong } from "@/lib/utils"

type Tab = "summary" | "transcript" | "actions" | "recording"

export function MeetingDetailPage() {
	const { id } = useParams({ from: "/meetings/$id" })
	const navigate = useNavigate()
	const [tab, setTab] = useState<Tab>("summary")

	const [meeting, setMeeting] = useState<Meeting | null>(null)
	const [transcript, setTranscript] = useState<Transcript | null>(null)
	const [segments, setSegments] = useState<Segment[]>([])
	const [summary, setSummary] = useState<Summary | null>(null)
	const [actions, setActions] = useState<ActionItem[]>([])
	const [loading, setLoading] = useState(true)
	const [editingTitle, setEditingTitle] = useState(false)
	const [titleDraft, setTitleDraft] = useState("")
	const [deleting, setDeleting] = useState(false)
	const [confirmingDelete, setConfirmingDelete] = useState(false)

	const load = useCallback(async () => {
		const [m, t, s, a] = await Promise.all([
			getMeeting(id),
			getTranscript(id),
			getSummary(id),
			getActionItems(id),
		])
		setMeeting(m)
		setTranscript(t)
		setSummary(s)
		setActions(a)
		if (t) setSegments(await getSegments(t.id))
		setLoading(false)
	}, [id])

	useEffect(() => {
		load()
	}, [load])

	async function handleTitleSave() {
		if (!meeting || !titleDraft.trim()) return
		await updateMeeting(id, { title: titleDraft.trim() })
		setMeeting((m) => (m ? { ...m, title: titleDraft.trim() } : m))
		setEditingTitle(false)
	}

	async function handleToggleAction(actionId: string, done: boolean) {
		await toggleActionItem(actionId, !done)
		setActions((prev) => prev.map((a) => (a.id === actionId ? { ...a, done: !done } : a)))
	}

	async function handleDeleteMeeting() {
		if (!meeting || deleting) return

		setDeleting(true)
		try {
			await deleteMeetingAndAssets(meeting.id)
			toast.success("Meeting deleted")
			navigate({ to: "/" })
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error)
			toast.error("Couldn't delete meeting", { description: message })
			setDeleting(false)
		}
	}

	if (loading) {
		return (
			<div className="flex h-full items-center justify-center">
				<span className="text-sm text-[var(--color-text-muted)]">Loading…</span>
			</div>
		)
	}

	if (!meeting) {
		return (
			<div className="flex h-full flex-col items-center justify-center gap-3">
				<p className="text-sm text-[var(--color-text-muted)]">Meeting not found.</p>
				<Link to="/" className="text-xs text-[var(--color-accent)] hover:underline">
					← Back to meetings
				</Link>
			</div>
		)
	}

	const tabs: { key: Tab; label: string }[] = [
		{ key: "summary", label: "Summary" },
		{ key: "transcript", label: "Transcript" },
		{ key: "actions", label: `Action Items${actions.length > 0 ? ` (${actions.length})` : ""}` },
		{ key: "recording", label: "Recording" },
	]

	return (
		<div className="flex flex-col h-full">
			{/* Header */}
			<div className="px-6 pt-5 pb-0 border-b border-[var(--color-border)]">
				<Link
					to="/"
					className="text-xs text-[var(--color-text-muted)] hover:text-[var(--color-text)] flex items-center gap-1 mb-3 w-fit transition-colors"
				>
					<svg
						aria-hidden="true"
						className="w-3 h-3"
						fill="none"
						viewBox="0 0 24 24"
						stroke="currentColor"
						strokeWidth={2}
					>
						<path strokeLinecap="round" strokeLinejoin="round" d="M15 19l-7-7 7-7" />
					</svg>
					All meetings
				</Link>

				{/* Title — inline editable */}
				{editingTitle ? (
					<div className="flex items-center gap-2 mb-1">
						<input
							// biome-ignore lint/a11y/noAutofocus: intentional focus on edit
							autoFocus
							value={titleDraft}
							onChange={(e) => setTitleDraft(e.target.value)}
							onBlur={handleTitleSave}
							onKeyDown={(e) => {
								if (e.key === "Enter") handleTitleSave()
								if (e.key === "Escape") setEditingTitle(false)
							}}
							className="flex-1 bg-transparent text-lg font-semibold text-[var(--color-text)] border-b border-[var(--color-accent)] outline-none pb-0.5"
						/>
					</div>
				) : (
					<button
						type="button"
						onClick={() => {
							setTitleDraft(meeting.title)
							setEditingTitle(true)
						}}
						className="text-lg font-semibold text-[var(--color-text)] hover:text-[var(--color-accent)] transition-colors text-left mb-1 w-full truncate"
					>
						{meeting.title}
					</button>
				)}

				<div className="mb-4 flex items-center justify-between gap-3">
					<p className="text-xs text-[var(--color-text-muted)]">
						{formatDate(meeting.startedAt)}
						{meeting.durationSeconds != null && (
							<> · {formatDurationLong(meeting.durationSeconds)}</>
						)}
					</p>
					<button
						type="button"
						onClick={() => setConfirmingDelete(true)}
						disabled={deleting}
						className="rounded-[var(--radius-sm)] border border-[var(--color-border)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-destructive)] hover:bg-[var(--color-destructive)]/10 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
					>
						{deleting ? "Deleting…" : "Delete Meeting"}
					</button>
				</div>

				{/* Tabs */}
				<div className="flex gap-1 -mb-px">
					{tabs.map((t) => (
						<button
							key={t.key}
							type="button"
							onClick={() => setTab(t.key)}
							className={`px-3 py-2 text-xs font-medium border-b-2 transition-colors ${
								tab === t.key
									? "border-[var(--color-accent)] text-[var(--color-text)]"
									: "border-transparent text-[var(--color-text-muted)] hover:text-[var(--color-text)]"
							}`}
						>
							{t.label}
						</button>
					))}
				</div>
			</div>

			{/* Tab content */}
			<div className="flex-1 overflow-y-auto">
				{confirmingDelete && (
					<div className="mx-6 mt-5 rounded-[var(--radius-md)] border border-[var(--color-destructive)]/30 bg-[var(--color-destructive)]/8 p-4">
						<p className="text-sm font-medium text-[var(--color-text)]">
							Delete this meeting and all associated local data?
						</p>
						<p className="mt-1 text-xs leading-relaxed text-[var(--color-text-muted)]">
							This removes the meeting row, transcript, summary, action items, embeddings, and
							matching local recording and summary files from your Mac.
						</p>
						<div className="mt-3 flex items-center gap-2">
							<button
								type="button"
								onClick={handleDeleteMeeting}
								disabled={deleting}
								className="rounded-[var(--radius-sm)] bg-[var(--color-destructive)] px-3 py-1.5 text-xs font-medium text-white hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed transition-opacity"
							>
								{deleting ? "Deleting…" : "Yes, delete it"}
							</button>
							<button
								type="button"
								onClick={() => setConfirmingDelete(false)}
								disabled={deleting}
								className="rounded-[var(--radius-sm)] border border-[var(--color-border)] px-3 py-1.5 text-xs text-[var(--color-text-muted)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface)] disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
							>
								Cancel
							</button>
						</div>
					</div>
				)}
				{tab === "summary" && <SummaryTab summary={summary} />}
				{tab === "transcript" && <TranscriptTab transcript={transcript} segments={segments} />}
				{tab === "actions" && <ActionsTab actions={actions} onToggle={handleToggleAction} />}
				{tab === "recording" && <RecordingTab meetingId={meeting.id} />}
			</div>
		</div>
	)
}

// ─── Tab panels ──────────────────────────────────────────────────────────────

function SummaryTab({ summary }: { summary: Summary | null }) {
	if (!summary) {
		return (
			<div className="flex flex-col items-center justify-center h-full gap-2 text-center px-8">
				<p className="text-sm text-[var(--color-text-muted)]">No summary yet.</p>
				<p className="text-xs text-[var(--color-text-muted)]">
					Add an AI provider in Settings to generate summaries automatically.
				</p>
			</div>
		)
	}

	return (
		<div className="px-6 py-5 max-w-2xl">
			<div
				className="prose prose-sm prose-invert max-w-none
				[&_h2]:text-sm [&_h2]:font-semibold [&_h2]:text-[var(--color-text)] [&_h2]:mt-5 [&_h2]:mb-2
				[&_p]:text-[var(--color-text-muted)] [&_p]:leading-relaxed
				[&_ul]:text-[var(--color-text-muted)] [&_li]:my-0.5
				[&_a]:text-[var(--color-accent)]
				[&_input[type=checkbox]]:accent-[var(--color-accent)]"
			>
				<ReactMarkdown remarkPlugins={[remarkGfm]}>{summary.markdownContent}</ReactMarkdown>
			</div>
		</div>
	)
}

function TranscriptTab({
	transcript,
	segments,
}: {
	transcript: Transcript | null
	segments: Segment[]
}) {
	if (!transcript) {
		return (
			<div className="flex items-center justify-center h-full">
				<p className="text-sm text-[var(--color-text-muted)]">No transcript yet.</p>
			</div>
		)
	}

	function msToTimestamp(ms: number): string {
		const s = Math.floor(ms / 1000)
		const m = Math.floor(s / 60)
		return `${String(m).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`
	}

	return (
		<div className="px-6 py-5 max-w-2xl">
			{segments.length > 0 ? (
				<div className="space-y-3">
					{segments.map((seg) => (
						<div key={seg.id} className="flex gap-3">
							<span className="text-[10px] font-mono text-[var(--color-text-muted)] pt-0.5 w-10 flex-shrink-0 tabular-nums">
								{msToTimestamp(seg.startMs)}
							</span>
							<p className="text-sm text-[var(--color-text)] leading-relaxed">{seg.text}</p>
						</div>
					))}
				</div>
			) : (
				<p className="text-sm text-[var(--color-text)] leading-relaxed whitespace-pre-wrap">
					{transcript.rawText}
				</p>
			)}
		</div>
	)
}

function ActionsTab({
	actions,
	onToggle,
}: {
	actions: ActionItem[]
	onToggle: (id: string, done: boolean) => void
}) {
	const pending = actions.filter((a) => !a.done)
	const completed = actions.filter((a) => a.done)

	if (actions.length === 0) {
		return (
			<div className="flex items-center justify-center h-full">
				<p className="text-sm text-[var(--color-text-muted)]">No action items found.</p>
			</div>
		)
	}

	return (
		<div className="px-6 py-5 max-w-xl space-y-6">
			{pending.length > 0 && (
				<section>
					<h3 className="text-xs font-medium text-[var(--color-text-muted)] uppercase tracking-wider mb-3">
						To do · {pending.length}
					</h3>
					<ul className="space-y-2">
						{pending.map((a) => (
							<ActionRow key={a.id} action={a} onToggle={onToggle} />
						))}
					</ul>
				</section>
			)}
			{completed.length > 0 && (
				<section>
					<h3 className="text-xs font-medium text-[var(--color-text-muted)] uppercase tracking-wider mb-3">
						Done · {completed.length}
					</h3>
					<ul className="space-y-2 opacity-60">
						{completed.map((a) => (
							<ActionRow key={a.id} action={a} onToggle={onToggle} />
						))}
					</ul>
				</section>
			)}
		</div>
	)
}

function ActionRow({
	action,
	onToggle,
}: {
	action: ActionItem
	onToggle: (id: string, done: boolean) => void
}) {
	return (
		<li className="flex items-start gap-3">
			<button
				type="button"
				onClick={() => onToggle(action.id, action.done)}
				className={`mt-0.5 flex-shrink-0 w-4 h-4 rounded border transition-colors ${
					action.done
						? "bg-[var(--color-success)] border-[var(--color-success)]"
						: "border-[var(--color-border)] hover:border-[var(--color-text-muted)]"
				}`}
			>
				{action.done && (
					<svg aria-hidden="true" viewBox="0 0 12 12" fill="none" className="w-full h-full p-0.5">
						<path
							d="M2 6l3 3 5-5"
							stroke="white"
							strokeWidth={1.5}
							strokeLinecap="round"
							strokeLinejoin="round"
						/>
					</svg>
				)}
			</button>
			<span
				className={`text-sm leading-relaxed ${action.done ? "line-through text-[var(--color-text-muted)]" : "text-[var(--color-text)]"}`}
			>
				{action.text}
			</span>
		</li>
	)
}

function RecordingTab({ meetingId }: { meetingId: string }) {
	const audioRef = useRef<HTMLAudioElement>(null)
	const [src, setSrc] = useState<string | null>(null)
	const [error, setError] = useState<string | null>(null)

	useEffect(() => {
		let objectUrl: string | null = null
		let cancelled = false

		async function loadRecording() {
			setSrc(null)
			setError(null)

			try {
				const recording = await getRecording(meetingId)
				if (!recording) return

				const { readFile } = await import("@tauri-apps/plugin-fs")
				const bytes = await readFile(recording.filePath)
				if (cancelled) return

				const mimeType = recording.filePath.endsWith(".wav") ? "audio/wav" : "audio/webm"
				objectUrl = URL.createObjectURL(new Blob([bytes], { type: mimeType }))
				setSrc(objectUrl)
			} catch (err) {
				const message = err instanceof Error ? err.message : String(err)
				if (!cancelled) setError(message)
			}
		}

		loadRecording()

		return () => {
			cancelled = true
			if (objectUrl) URL.revokeObjectURL(objectUrl)
		}
	}, [meetingId])

	return (
		<div className="px-6 py-5 flex flex-col items-center gap-4">
			{src ? (
				<div className="w-full max-w-md space-y-2">
					{/* biome-ignore lint/a11y/useMediaCaption: local recording playback has no generated caption track yet */}
					<audio ref={audioRef} controls src={src} className="w-full" />
					<p className="text-xs text-[var(--color-text-muted)] text-center">
						Playback is loaded from the saved local `.webm` recording.
					</p>
				</div>
			) : (
				<div className="flex flex-col items-center gap-2 text-center">
					<div className="w-10 h-10 rounded-xl bg-[var(--color-surface)] flex items-center justify-center">
						<svg
							aria-hidden="true"
							className="w-5 h-5 text-[var(--color-text-muted)]"
							fill="none"
							viewBox="0 0 24 24"
							stroke="currentColor"
							strokeWidth={1.5}
						>
							<path
								strokeLinecap="round"
								strokeLinejoin="round"
								d="M19.114 5.636a9 9 0 010 12.728M16.463 8.288a5.25 5.25 0 010 7.424M6.75 8.25l4.72-4.72a.75.75 0 011.28.53v15.88a.75.75 0 01-1.28.53l-4.72-4.72H4.51c-.88 0-1.704-.507-1.938-1.354A9.01 9.01 0 012.25 12c0-.83.112-1.633.322-2.396C2.806 8.756 3.63 8.25 4.51 8.25H6.75z"
							/>
						</svg>
					</div>
					<p className="text-sm text-[var(--color-text-muted)]">
						{error ? "Couldn't load recording." : "No saved recording found yet."}
					</p>
					<p className="text-xs text-[var(--color-text-muted)]">
						{error ? error : "Find the file at ~/Library/Minutae/recordings/"}
					</p>
				</div>
			)}
		</div>
	)
}

// Tauri internals type shim
declare global {
	interface Window {
		__TAURI_INTERNALS__?: { metadata?: { currentDir?: string } }
	}
}
