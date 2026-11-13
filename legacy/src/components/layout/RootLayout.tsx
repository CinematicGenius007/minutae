import { Link, Outlet } from "@tanstack/react-router"
import { useCallback, useEffect, useState } from "react"
import { ChartIcon, MeetingsIcon, MicIcon, SearchIcon, SettingsIcon } from "@/components/icons"
import { MeetingPrompt } from "@/components/MeetingPrompt"
import { RecordingPill } from "@/components/RecordingPill"
import { SearchModal } from "@/components/SearchModal"
import { useMicMonitor } from "@/hooks/useMicMonitor"
import { useRecordingSession } from "@/hooks/useRecordingSession"
import { formatDuration } from "@/lib/utils"
import { useRecordingStore } from "@/store/recording"

const navItems = [
	{ to: "/", label: "Meetings", icon: MeetingsIcon },
	{ to: "/dictate", label: "Dictate", icon: MicIcon },
	{ to: "/usage", label: "Usage", icon: ChartIcon },
	{ to: "/settings", label: "Settings", icon: SettingsIcon },
] as const

export function RootLayout() {
	useMicMonitor()

	const micActive = useRecordingStore((s) => s.micActive)
	const status = useRecordingStore((s) => s.status)
	const [searchOpen, setSearchOpen] = useState(false)

	const session = useRecordingSession()

	const handleKeyDown = useCallback(
		(e: KeyboardEvent) => {
			const meta = e.metaKey || e.ctrlKey

			if (meta && !e.shiftKey && e.key === "k") {
				e.preventDefault()
				setSearchOpen((prev) => !prev)
			}

			if (meta && e.shiftKey && e.key.toLowerCase() === "r") {
				e.preventDefault()
				if (session.canStartManual) session.startManualSession()
			}
		},
		[session],
	)

	useEffect(() => {
		document.addEventListener("keydown", handleKeyDown)
		return () => document.removeEventListener("keydown", handleKeyDown)
	}, [handleKeyDown])

	const isLive = status === "recording" || status === "paused"
	const busy = isLive || status === "processing"

	return (
		<div className="flex h-screen w-screen overflow-hidden bg-bg text-text">
			<aside className="flex w-[232px] shrink-0 flex-col border-r border-border bg-surface">
				{/* Drag strip clears the macOS traffic lights */}
				<div data-tauri-drag-region className="h-12 shrink-0" />

				<div className="px-3 pb-3">
					<button
						type="button"
						onClick={() => session.startManualSession()}
						disabled={!session.canStartManual}
						title="Start a recording (⌘⇧R)"
						className="flex w-full items-center gap-2.5 rounded-sm bg-accent px-3 py-2 text-[13px] font-medium text-on-accent transition-colors hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-50"
					>
						<span className="size-2 rounded-full bg-on-accent" />
						<span className="flex-1 text-left">{busy ? "Recording…" : "Record"}</span>
						{!busy && <kbd className="font-sans text-[11px] opacity-70">⌘⇧R</kbd>}
					</button>

					<button
						type="button"
						onClick={() => setSearchOpen(true)}
						className="mt-2 flex w-full items-center gap-2.5 rounded-sm border border-border bg-bg px-3 py-2 text-[13px] text-text-muted transition-colors hover:border-text-muted"
					>
						<SearchIcon size={14} />
						<span className="flex-1 text-left">Search</span>
						<kbd className="font-sans text-[11px]">⌘K</kbd>
					</button>
				</div>

				<nav className="flex-1 space-y-0.5 px-3">
					{navItems.map((item) => (
						<Link
							key={item.to}
							to={item.to}
							activeOptions={{ exact: item.to === "/" }}
							className="flex items-center gap-2.5 rounded-sm px-3 py-2 text-[13px] font-medium text-text-muted transition-colors hover:bg-surface-raised hover:text-text"
							activeProps={{
								className:
									"flex items-center gap-2.5 rounded-sm px-3 py-2 text-[13px] font-medium bg-bg text-text shadow-[0_0_0_1px_var(--color-border)]",
							}}
						>
							<item.icon size={16} />
							{item.label}
						</Link>
					))}
				</nav>

				<div className="border-t border-border px-4 py-3.5 text-xs">
					{isLive ? (
						<div className="flex items-center gap-2 font-medium text-text">
							<span
								className={`size-2 rounded-full ${status === "recording" ? "live-dot" : "bg-text-muted"}`}
							/>
							{status === "recording" ? "Recording" : "Paused"}
							<span className="ml-auto tabular-nums text-text-muted">
								{formatDuration(session.durationSeconds)}
							</span>
						</div>
					) : status === "processing" ? (
						<div className="flex items-center gap-2 text-text-muted">
							<span className="size-2 animate-pulse rounded-full bg-accent" />
							Transcribing &amp; summarizing
						</div>
					) : (
						<div className="flex items-center gap-2 text-text-muted">
							<span className={`size-2 rounded-full ${micActive ? "bg-success" : "bg-border"}`} />
							{micActive ? "Mic in use by another app" : "Listening for meetings"}
						</div>
					)}
				</div>
			</aside>

			<main className="relative flex-1 overflow-y-auto">
				<div data-tauri-drag-region className="sticky top-0 z-10 h-6 w-full" />
				<div className="-mt-6">
					<Outlet />
				</div>
			</main>

			<MeetingPrompt onStartRecording={session.startRecording} />
			<RecordingPill
				durationSeconds={session.durationSeconds}
				audioLevel={session.audioLevel}
				pauseRecording={session.pauseRecording}
				resumeRecording={session.resumeRecording}
				stopRecording={session.stopRecording}
			/>
			<SearchModal open={searchOpen} onClose={() => setSearchOpen(false)} />
		</div>
	)
}
