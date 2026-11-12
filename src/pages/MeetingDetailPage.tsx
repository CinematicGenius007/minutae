import { Link, useNavigate, useParams } from "@tanstack/react-router"
import { useCallback, useEffect, useRef, useState } from "react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import { toast } from "sonner"
import { ArrowLeftIcon, CopyIcon, RefreshIcon, TrashIcon } from "@/components/icons"
import { getActionItems, toggleActionItem } from "@/lib/db/action-items"
import { deleteMeetingAndAssets, getMeeting, updateMeeting } from "@/lib/db/meetings"
import { getRecording } from "@/lib/db/recordings"
import type { ActionItem, Meeting, Segment, Summary, Transcript } from "@/lib/db/schema"
import { getSummary } from "@/lib/db/summaries"
import { getSegments, getTranscript } from "@/lib/db/transcripts"
import { formatDateTime, formatDurationLong } from "@/lib/utils"
import { useSummaryStore } from "@/store/summary"
import { useTranscriptionStore } from "@/store/transcription"

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

	const transcriptionJobs = useTranscriptionStore((s) => s.jobs)
	const summaryJobs = useSummaryStore((s) => s.jobs)
	const jobKey = [...transcriptionJobs, ...summaryJobs]
		.filter((j) => j.meetingId === id)
		.map((j) => j.status)
		.join("|")

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
		setSegments(t ? await getSegments(t.id) : [])
		setLoading(false)
	}, [id])

	// biome-ignore lint/correctness/useExhaustiveDependencies: jobKey re-loads when the pipeline advances
	useEffect(() => {
		load()
	}, [load, jobKey])

	async function handleTitleSave() {
		if (!meeting || !titleDraft.trim()) return setEditingTitle(false)
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
			toast.error("Couldn't delete meeting", {
				description: error instanceof Error ? error.message : String(error),
			})
			setDeleting(false)
		}
	}

	async function copy(label: string, text: string) {
		try {
			await navigator.clipboard.writeText(text)
			toast.success(`${label} copied`)
		} catch {
			toast.error("Couldn't copy to the clipboard")
		}
	}

	/** Re-run whatever stage is missing: transcription first, then the summary. */
	async function handleRetry() {
		if (!meeting) return
		try {
			if (!transcript) {
				const recording = await getRecording(meeting.id)
				if (!recording) {
					toast.error("No saved audio for this meeting, so there is nothing to retry.")
					return
				}
				await updateMeeting(meeting.id, { status: "processing" })
				useTranscriptionStore
					.getState()
					.enqueue({ meetingId: meeting.id, audioPath: recording.filePath })
			} else {
				await updateMeeting(meeting.id, { status: "processing" })
				useSummaryStore
					.getState()
					.enqueue({ meetingId: meeting.id, transcript: transcript.rawText })
			}
			await load()
		} catch (error) {
			toast.error("Couldn't retry", {
				description: error instanceof Error ? error.message : String(error),
			})
		}
	}

	if (loading) return null

	if (!meeting) {
		return (
			<div className="flex h-full flex-col items-center justify-center gap-3">
				<p className="text-sm text-text-muted">Meeting not found.</p>
				<Link to="/" className="text-[13px] font-medium text-accent hover:underline">
					Back to meetings
				</Link>
			</div>
		)
	}

	const openActions = actions.filter((a) => !a.done).length
	const tabs: { key: Tab; label: string }[] = [
		{ key: "summary", label: "Summary" },
		{ key: "transcript", label: "Transcript" },
		{ key: "actions", label: openActions > 0 ? `Actions · ${openActions}` : "Actions" },
		{ key: "recording", label: "Audio" },
	]
	const needsAttention =
		meeting.status === "failed" || (meeting.status === "processing" && !summary && jobKey === "")

	return (
		<div className="mx-auto max-w-3xl px-8 pt-14 pb-24">
			<Link
				to="/"
				className="mb-6 inline-flex items-center gap-1.5 text-[13px] font-medium text-text-muted transition-colors hover:text-text"
			>
				<ArrowLeftIcon size={14} />
				Meetings
			</Link>

			{editingTitle ? (
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
					className="w-full border-b-2 border-accent bg-transparent pb-1 text-[28px] font-semibold tracking-tight text-text outline-none"
				/>
			) : (
				<h1 className="text-[28px] font-semibold tracking-tight text-text">
					<button
						type="button"
						title="Rename"
						onClick={() => {
							setTitleDraft(meeting.title)
							setEditingTitle(true)
						}}
						className="w-full cursor-text text-left hover:text-accent"
					>
						{meeting.title}
					</button>
				</h1>
			)}

			<div className="mt-2 flex items-center justify-between gap-4">
				<p className="text-[13px] text-text-muted">
					{formatDateTime(meeting.startedAt)}
					{meeting.durationSeconds != null && <> · {formatDurationLong(meeting.durationSeconds)}</>}
				</p>
				<div className="flex items-center gap-1">
					{summary && (
						<IconButton
							label="Copy summary"
							onClick={() => copy("Summary", summary.markdownContent)}
						>
							<CopyIcon size={15} />
						</IconButton>
					)}
					<IconButton label="Delete meeting" onClick={() => setConfirmingDelete(true)} danger>
						<TrashIcon size={15} />
					</IconButton>
				</div>
			</div>

			{confirmingDelete && (
				<div className="mt-5 rounded-md border border-destructive/40 bg-surface p-4">
					<p className="text-[14px] font-medium text-text">Delete this meeting?</p>
					<p className="mt-1 text-[13px] leading-relaxed text-text-muted">
						The transcript, summary, actions, search index and the saved audio are removed from this
						Mac. This can’t be undone.
					</p>
					<div className="mt-3 flex gap-2">
						<button
							type="button"
							onClick={handleDeleteMeeting}
							disabled={deleting}
							className="rounded-sm bg-destructive px-3 py-1.5 text-[13px] font-medium text-on-accent disabled:opacity-50"
						>
							{deleting ? "Deleting…" : "Delete"}
						</button>
						<button
							type="button"
							onClick={() => setConfirmingDelete(false)}
							className="rounded-sm px-3 py-1.5 text-[13px] font-medium text-text-muted hover:bg-surface-raised"
						>
							Cancel
						</button>
					</div>
				</div>
			)}

			{needsAttention && (
				<div className="mt-6 flex items-center justify-between gap-4 rounded-md bg-surface px-4 py-3">
					<p className="text-[13px] text-text-muted">
						{transcript
							? "The summary didn’t finish."
							: "This meeting hasn’t been transcribed yet."}
					</p>
					<button
						type="button"
						onClick={handleRetry}
						className="flex items-center gap-1.5 rounded-sm bg-accent px-3 py-1.5 text-[13px] font-medium text-on-accent hover:bg-accent-hover"
					>
						<RefreshIcon size={14} />
						{transcript ? "Generate summary" : "Transcribe"}
					</button>
				</div>
			)}

			<div role="tablist" className="mt-8 flex gap-6 border-b border-border">
				{tabs.map((t) => (
					<button
						key={t.key}
						type="button"
						role="tab"
						aria-selected={tab === t.key}
						onClick={() => setTab(t.key)}
						className={`-mb-px border-b-2 pb-2.5 text-[13px] font-medium transition-colors ${
							tab === t.key
								? "border-accent text-text"
								: "border-transparent text-text-muted hover:text-text"
						}`}
					>
						{t.label}
					</button>
				))}
			</div>

			<div className="pt-8">
				{tab === "summary" && <SummaryTab summary={summary} />}
				{tab === "transcript" && (
					<TranscriptTab
						transcript={transcript}
						segments={segments}
						onCopy={() => transcript && copy("Transcript", transcript.rawText)}
					/>
				)}
				{tab === "actions" && <ActionsTab actions={actions} onToggle={handleToggleAction} />}
				{tab === "recording" && <RecordingTab meetingId={meeting.id} />}
			</div>
		</div>
	)
}

function IconButton({
	label,
	onClick,
	danger,
	children,
}: {
	label: string
	onClick: () => void
	danger?: boolean
	children: React.ReactNode
}) {
	return (
		<button
			type="button"
			title={label}
			aria-label={label}
			onClick={onClick}
			className={`grid size-8 place-items-center rounded-sm text-text-muted transition-colors hover:bg-surface ${danger ? "hover:text-destructive" : "hover:text-text"}`}
		>
			{children}
		</button>
	)
}

// ─── Tab panels ──────────────────────────────────────────────────────────────

function Empty({ title, hint }: { title: string; hint?: string | undefined }) {
	return (
		<div className="py-16 text-center">
			<p className="text-[14px] font-medium text-text">{title}</p>
			{hint && <p className="mt-1 text-[13px] text-text-muted">{hint}</p>}
		</div>
	)
}

function SummaryTab({ summary }: { summary: Summary | null }) {
	if (!summary) {
		return (
			<Empty
				title="No summary yet"
				hint="Add a summary provider key in Settings, then generate one."
			/>
		)
	}

	return (
		<div className="md selectable max-w-2xl">
			<ReactMarkdown remarkPlugins={[remarkGfm]}>{summary.markdownContent}</ReactMarkdown>
		</div>
	)
}

function TranscriptTab({
	transcript,
	segments,
	onCopy,
}: {
	transcript: Transcript | null
	segments: Segment[]
	onCopy: () => void
}) {
	if (!transcript) return <Empty title="No transcript yet" />

	function stamp(ms: number): string {
		const total = Math.floor(ms / 1000)
		return `${String(Math.floor(total / 60)).padStart(2, "0")}:${String(total % 60).padStart(2, "0")}`
	}

	return (
		<div className="selectable max-w-2xl">
			<button
				type="button"
				onClick={onCopy}
				className="mb-5 flex items-center gap-1.5 text-[13px] font-medium text-text-muted hover:text-text"
			>
				<CopyIcon size={14} />
				Copy transcript
			</button>
			{segments.length > 0 ? (
				<div className="space-y-4">
					{segments.map((seg) => (
						<div key={seg.id} className="flex gap-4">
							<span className="w-11 shrink-0 pt-[3px] font-mono text-xs text-text-muted tabular-nums">
								{stamp(seg.startMs)}
							</span>
							<p className="text-[15px] leading-relaxed text-text">{seg.text}</p>
						</div>
					))}
				</div>
			) : (
				<p className="text-[15px] leading-relaxed whitespace-pre-wrap text-text">
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
	if (actions.length === 0)
		return <Empty title="No action items" hint="Nothing was assigned in this meeting." />

	const ordered = [...actions].sort((x, y) => Number(x.done) - Number(y.done))

	return (
		<ul className="max-w-2xl divide-y divide-border border-y border-border">
			{ordered.map((a) => (
				<li key={a.id} className="flex items-start gap-3 py-3.5">
					<input
						type="checkbox"
						checked={a.done}
						aria-label={a.text}
						onChange={() => onToggle(a.id, a.done)}
						className="mt-[3px] size-[18px] shrink-0 cursor-pointer accent-[var(--color-accent)]"
					/>
					<span
						className={`selectable text-[15px] leading-relaxed ${a.done ? "text-text-muted line-through" : "text-text"}`}
					>
						{a.text}
					</span>
				</li>
			))}
		</ul>
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

	if (src) {
		return (
			<div className="max-w-2xl space-y-2">
				{/* biome-ignore lint/a11y/useMediaCaption: the transcript tab is the caption track */}
				<audio ref={audioRef} controls src={src} className="w-full" />
				<p className="text-xs text-text-muted">
					Stored locally as a 16 kHz WAV in Application Support.
				</p>
			</div>
		)
	}

	return (
		<Empty
			title={error ? "Couldn’t load the recording" : "No audio saved for this meeting"}
			hint={error ?? undefined}
		/>
	)
}

// Tauri internals type shim
declare global {
	interface Window {
		__TAURI_INTERNALS__?: { metadata?: { currentDir?: string } }
	}
}
