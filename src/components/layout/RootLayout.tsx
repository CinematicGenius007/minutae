import { Link, Outlet } from "@tanstack/react-router"
import { useCallback, useEffect, useState } from "react"
import { MeetingPrompt } from "@/components/MeetingPrompt"
import { RecordingPill } from "@/components/RecordingPill"
import { SearchModal } from "@/components/SearchModal"
import { useMicMonitor } from "@/hooks/useMicMonitor"
import { useRecordingSession } from "@/hooks/useRecordingSession"
import { useRecordingStore } from "@/store/recording"

const navItems = [
	{ to: "/", label: "Meetings" },
	{ to: "/usage", label: "Usage" },
	{ to: "/settings", label: "Settings" },
] as const

export function RootLayout() {
	useMicMonitor()

	const micActive = useRecordingStore((s) => s.micActive)
	const status = useRecordingStore((s) => s.status)
	const [searchOpen, setSearchOpen] = useState(false)

	const session = useRecordingSession()

	// Global keyboard shortcuts
	const handleKeyDown = useCallback(
		(e: KeyboardEvent) => {
			const meta = e.metaKey || e.ctrlKey

			// Cmd+K → search
			if (meta && !e.shiftKey && e.key === "k") {
				e.preventDefault()
				setSearchOpen((prev) => !prev)
			}

			// Cmd+Shift+R → new recording (only when not already recording)
			if (meta && e.shiftKey && e.key === "r") {
				e.preventDefault()
				if (session.canStartManual) {
					session.startManualSession()
				}
			}
		},
		[session],
	)

	useEffect(() => {
		document.addEventListener("keydown", handleKeyDown)
		return () => document.removeEventListener("keydown", handleKeyDown)
	}, [handleKeyDown])

	const isRecording = status === "recording" || status === "paused" || status === "processing"

	return (
		<div className="flex h-screen w-screen overflow-hidden bg-bg">
			{/* Sidebar */}
			<aside className="w-60 shrink-0 border-r border-border flex flex-col">
				{/* Title bar drag area + app name */}
				{/* pl-20 reserves space for macOS traffic lights (~80px) */}
				<div className="h-10" />
				{/* <div
					className="h-8 flex justify-end items-center pr-2 select-none"
					style={{ WebkitAppRegion: "drag" } as React.CSSProperties}
				>
					<span className="font-semibold text-text tracking-tight">Minutae</span>
				</div> */}

				{/* New Recording button */}
				<div className="px-2 pb-1">
					<button
						type="button"
						onClick={() => session.startManualSession()}
						disabled={!session.canStartManual}
						title="New Recording (⌘⇧R)"
						className="w-full flex items-center gap-2 px-3 py-2 rounded-sm text-sm font-medium bg-accent text-white hover:opacity-90 transition-opacity disabled:opacity-40 disabled:cursor-not-allowed"
					>
						<svg
							aria-hidden="true"
							className="w-3.5 h-3.5 shrink-0"
							viewBox="0 0 24 24"
							fill="currentColor"
						>
							<circle cx="12" cy="12" r="6" />
						</svg>
						<span className="flex-1 text-left">{isRecording ? "Recording…" : "New Recording"}</span>
						{!isRecording && (
							<kbd className="text-[9px] opacity-70 border border-white/30 rounded px-1 py-0.5">
								⌘⇧R
							</kbd>
						)}
					</button>
				</div>

				{/* Search button */}
				<div className="px-2 pb-1">
					<button
						type="button"
						onClick={() => setSearchOpen(true)}
						className="w-full flex items-center gap-2 px-3 py-2 rounded-sm text-sm text-text-muted hover:bg-surface hover:text-text transition-colors"
					>
						<svg
							aria-hidden="true"
							className="w-3.5 h-3.5 shrink-0"
							fill="none"
							viewBox="0 0 24 24"
							stroke="currentColor"
							strokeWidth={2}
						>
							<circle cx="11" cy="11" r="8" />
							<path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-4.35-4.35" />
						</svg>
						<span className="flex-1 text-left">Search</span>
						<kbd className="text-[10px] border border-border rounded px-1.5 py-0.5">⌘K</kbd>
					</button>
				</div>

				{/* Nav */}
				<nav className="flex-1 px-2 py-1 space-y-0.5">
					{navItems.map((item) => (
						<Link
							key={item.to}
							to={item.to}
							className="block px-3 py-2 rounded-sm text-sm text-text-muted hover:bg-surface hover:text-text transition-colors"
							activeProps={{
								className: "block px-3 py-2 rounded-sm text-sm bg-surface text-text",
							}}
							activeOptions={{ exact: item.to === "/" }}
						>
							{item.label}
						</Link>
					))}
				</nav>

				{/* Mic / recording status */}
				<div className="px-4 py-4 border-t border-border">
					{status === "recording" || status === "paused" ? (
						<div className="flex items-center gap-2 text-xs text-accent">
							<span className="relative flex h-2 w-2">
								{status === "recording" && (
									<span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-accent opacity-75" />
								)}
								<span className="relative inline-flex h-2 w-2 rounded-full bg-accent" />
							</span>
							{status === "recording" ? "Recording" : "Paused"}
						</div>
					) : (
						<div className="flex items-center gap-2 text-xs text-text-muted">
							<span
								className={`w-2 h-2 rounded-full transition-colors ${
									micActive ? "bg-success" : "bg-border"
								}`}
							/>
							{micActive ? "Mic active" : "Idle"}
						</div>
					)}
				</div>
			</aside>

			{/* Main content */}
			<main className="flex-1 overflow-y-auto">
				<Outlet />
			</main>

			{/* Floating overlays */}
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
