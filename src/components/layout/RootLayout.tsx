import { Link, Outlet } from "@tanstack/react-router"
import { useCallback, useEffect, useState } from "react"
import { MeetingPrompt } from "@/components/MeetingPrompt"
import { RecordingPill } from "@/components/RecordingPill"
import { SearchModal } from "@/components/SearchModal"
import { useMicMonitor } from "@/hooks/useMicMonitor"
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

	// Global Cmd+K shortcut
	const handleKeyDown = useCallback((e: KeyboardEvent) => {
		if ((e.metaKey || e.ctrlKey) && e.key === "k") {
			e.preventDefault()
			setSearchOpen((prev) => !prev)
		}
	}, [])

	useEffect(() => {
		document.addEventListener("keydown", handleKeyDown)
		return () => document.removeEventListener("keydown", handleKeyDown)
	}, [handleKeyDown])

	return (
		<div className="flex h-screen w-screen overflow-hidden bg-[var(--color-bg)]">
			{/* Sidebar */}
			<aside className="w-60 flex-shrink-0 border-r border-[var(--color-border)] flex flex-col">
				{/* Title bar drag area + app name */}
				{/* pl-20 reserves space for macOS traffic lights (~80px) */}
				<div
					className="h-12 flex items-center pl-20 pr-4 select-none"
					style={{ WebkitAppRegion: "drag" } as React.CSSProperties}
				>
					<span className="font-semibold text-[var(--color-text)] tracking-tight">Minutae</span>
				</div>

				{/* Search button */}
				<div className="px-2 pb-1">
					<button
						type="button"
						onClick={() => setSearchOpen(true)}
						className="w-full flex items-center gap-2 px-3 py-2 rounded-[var(--radius-sm)] text-sm text-[var(--color-text-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-text)] transition-colors"
					>
						<svg
							aria-hidden="true"
							className="w-3.5 h-3.5 flex-shrink-0"
							fill="none"
							viewBox="0 0 24 24"
							stroke="currentColor"
							strokeWidth={2}
						>
							<circle cx="11" cy="11" r="8" />
							<path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-4.35-4.35" />
						</svg>
						<span className="flex-1 text-left">Search</span>
						<kbd className="text-[10px] border border-[var(--color-border)] rounded px-1.5 py-0.5">
							⌘K
						</kbd>
					</button>
				</div>

				{/* Nav */}
				<nav className="flex-1 px-2 py-1 space-y-0.5">
					{navItems.map((item) => (
						<Link
							key={item.to}
							to={item.to}
							className="block px-3 py-2 rounded-[var(--radius-sm)] text-sm text-[var(--color-text-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-text)] transition-colors"
							activeProps={{
								className:
									"block px-3 py-2 rounded-[var(--radius-sm)] text-sm bg-[var(--color-surface)] text-[var(--color-text)]",
							}}
							activeOptions={{ exact: item.to === "/" }}
						>
							{item.label}
						</Link>
					))}
				</nav>

				{/* Mic / recording status */}
				<div className="px-4 py-4 border-t border-[var(--color-border)]">
					{status === "recording" || status === "paused" ? (
						<div className="flex items-center gap-2 text-xs text-[var(--color-accent)]">
							<span className="relative flex h-2 w-2">
								{status === "recording" && (
									<span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-[var(--color-accent)] opacity-75" />
								)}
								<span className="relative inline-flex h-2 w-2 rounded-full bg-[var(--color-accent)]" />
							</span>
							{status === "recording" ? "Recording" : "Paused"}
						</div>
					) : (
						<div className="flex items-center gap-2 text-xs text-[var(--color-text-muted)]">
							<span
								className={`w-2 h-2 rounded-full transition-colors ${
									micActive ? "bg-[var(--color-success)]" : "bg-[var(--color-border)]"
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
			<MeetingPrompt />
			<RecordingPill />
			<SearchModal open={searchOpen} onClose={() => setSearchOpen(false)} />
		</div>
	)
}
