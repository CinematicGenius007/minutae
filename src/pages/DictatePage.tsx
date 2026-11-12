import { useEffect, useState } from "react"
import { toast } from "sonner"
import { CopyIcon, MicIcon, StopIcon, TrashIcon } from "@/components/icons"
import { useDictation } from "@/hooks/useDictation"
import { formatDuration } from "@/lib/utils"

export function DictatePage() {
	const d = useDictation()
	const [copiedId, setCopiedId] = useState<string | null>(null)

	// Space toggles dictation unless the user is typing in a field.
	useEffect(() => {
		function onKey(e: KeyboardEvent) {
			const el = e.target as HTMLElement | null
			if (e.code !== "Space" || el?.closest("input, textarea, button, [contenteditable]")) return
			e.preventDefault()
			if (d.phase === "idle") void d.start()
			else if (d.phase === "listening") void d.stop()
		}
		document.addEventListener("keydown", onKey)
		return () => document.removeEventListener("keydown", onKey)
	}, [d])

	async function copy(id: string, text: string) {
		try {
			await navigator.clipboard.writeText(text)
			setCopiedId(id)
			setTimeout(() => setCopiedId((c) => (c === id ? null : c)), 1500)
		} catch {
			toast.error("Couldn’t copy to the clipboard")
		}
	}

	const listening = d.phase === "listening"
	const working = d.phase === "transcribing"
	const ring = listening ? 1 + Math.min(0.5, d.level * 4) : 1

	return (
		<div className="mx-auto max-w-2xl px-8 pt-14 pb-24">
			<h1 className="text-[28px] font-semibold tracking-tight text-text">Dictate</h1>
			<p className="mt-1 text-[13px] text-text-muted">
				Speak, get text. Transcribed on this Mac; the audio is deleted right after.
			</p>

			<div className="mt-12 flex flex-col items-center">
				<div className="relative grid size-28 place-items-center">
					<span
						className="absolute inset-0 rounded-full bg-live/15 transition-transform duration-100"
						style={{ transform: `scale(${ring})`, opacity: listening ? 1 : 0 }}
					/>
					<button
						type="button"
						onClick={() => (listening ? d.stop() : d.start())}
						disabled={working || d.meetingBusy}
						aria-label={listening ? "Stop dictation" : "Start dictation"}
						className={`relative grid size-20 place-items-center rounded-full transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
							listening
								? "bg-live text-white"
								: "bg-text text-bg hover:bg-accent hover:text-on-accent"
						}`}
					>
						{listening ? <StopIcon size={22} /> : <MicIcon size={26} />}
					</button>
				</div>

				<p className="mt-5 h-5 text-[13px] text-text-muted" aria-live="polite">
					{d.meetingBusy ? (
						"A meeting is being recorded. Dictation is paused."
					) : listening ? (
						`Listening · ${formatDuration(d.seconds)}`
					) : working ? (
						"Transcribing…"
					) : (
						<>
							Press{" "}
							<kbd className="rounded border border-border px-1.5 py-0.5 font-sans text-xs">
								Space
							</kbd>{" "}
							or click to start
						</>
					)}
				</p>
				{d.error && (
					<p className="mt-3 max-w-md text-center text-[13px] text-destructive">{d.error}</p>
				)}
			</div>

			{d.history.length > 0 && (
				<section className="mt-14">
					<div className="mb-1 flex items-baseline justify-between">
						<h2 className="text-[11px] font-semibold tracking-[0.08em] text-text-muted uppercase">
							Recent
						</h2>
						<button
							type="button"
							onClick={d.clear}
							className="text-xs font-medium text-text-muted hover:text-text"
						>
							Clear all
						</button>
					</div>
					<ul className="divide-y divide-border border-y border-border">
						{d.history.map((entry) => (
							<li key={entry.id} className="group flex items-start gap-4 py-4">
								<div className="min-w-0 flex-1">
									<p className="selectable text-[15px] leading-relaxed text-text">{entry.text}</p>
									<p className="mt-1 font-mono text-xs text-text-muted">
										{new Date(entry.createdAt).toLocaleTimeString("en-US", {
											hour: "numeric",
											minute: "2-digit",
										})}
									</p>
								</div>
								<div className="flex shrink-0 items-center gap-1 opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
									<button
										type="button"
										onClick={() => copy(entry.id, entry.text)}
										className="flex h-8 items-center gap-1.5 rounded-sm px-2 text-[13px] font-medium text-text-muted hover:bg-surface hover:text-text"
									>
										<CopyIcon size={14} />
										{copiedId === entry.id ? "Copied" : "Copy"}
									</button>
									<button
										type="button"
										aria-label="Delete"
										onClick={() => d.remove(entry.id)}
										className="grid size-8 place-items-center rounded-sm text-text-muted hover:bg-surface hover:text-destructive"
									>
										<TrashIcon size={14} />
									</button>
								</div>
							</li>
						))}
					</ul>
				</section>
			)}
		</div>
	)
}
