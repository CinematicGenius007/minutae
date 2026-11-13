import { useNavigate } from "@tanstack/react-router"
import { Command } from "cmdk"
import { useCallback, useEffect, useRef, useState } from "react"
import type { SearchResult } from "@/lib/db/embeddings"
import { ftsSearch, semanticSearch } from "@/lib/db/embeddings"
import { embed } from "@/lib/embeddings/embed"

interface Props {
	open: boolean
	onClose: () => void
}

export function SearchModal({ open, onClose }: Props) {
	const [query, setQuery] = useState("")
	const [results, setResults] = useState<SearchResult[]>([])
	const [searching, setSearching] = useState(false)
	const navigate = useNavigate()
	const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

	// `~` prefix = semantic mode
	const isSemantic = query.startsWith("~")
	const searchQuery = isSemantic ? query.slice(1).trim() : query.trim()

	const runSearch = useCallback(async (q: string, semantic: boolean) => {
		if (q.length < 2) {
			setResults([])
			return
		}
		setSearching(true)
		try {
			if (semantic) {
				const vec = await embed(q)
				setResults(await semanticSearch(vec))
			} else {
				setResults(await ftsSearch(q))
			}
		} catch {
			setResults([])
		} finally {
			setSearching(false)
		}
	}, [])

	// Debounce search as user types
	useEffect(() => {
		if (timerRef.current) clearTimeout(timerRef.current)
		timerRef.current = setTimeout(() => runSearch(searchQuery, isSemantic), 200)
		return () => {
			if (timerRef.current) clearTimeout(timerRef.current)
		}
	}, [searchQuery, isSemantic, runSearch])

	// Reset when closed
	useEffect(() => {
		if (!open) {
			setQuery("")
			setResults([])
		}
	}, [open])

	// Close on Escape
	const handleKeyDown = useCallback(
		(e: KeyboardEvent) => {
			if (e.key === "Escape") onClose()
		},
		[onClose],
	)

	useEffect(() => {
		if (open) document.addEventListener("keydown", handleKeyDown)
		return () => document.removeEventListener("keydown", handleKeyDown)
	}, [open, handleKeyDown])

	function handleSelect(meetingId: string) {
		navigate({ to: "/meetings/$id", params: { id: meetingId } })
		onClose()
	}

	if (!open) return null

	return (
		/* Backdrop — keyboard-dismissable via the global Escape listener in useEffect */
		// biome-ignore lint/a11y/noStaticElementInteractions: backdrop dismiss pattern
		// biome-ignore lint/a11y/useKeyWithClickEvents: Escape handled by global keydown listener
		<div
			className="fixed inset-0 z-[100] flex items-start justify-center pt-[18vh]"
			onClick={onClose}
		>
			<div className="absolute inset-0 bg-black/50 backdrop-blur-sm" />

			{/* Panel */}
			{/* biome-ignore lint/a11y/noStaticElementInteractions: stop-propagation trap on dialog container */}
			{/* biome-ignore lint/a11y/useKeyWithClickEvents: keyboard handled by Command component + global listener */}
			<div
				className="relative w-full max-w-lg mx-4 bg-[var(--color-surface)] border border-[var(--color-border)] rounded-xl shadow-2xl overflow-hidden"
				onClick={(e) => e.stopPropagation()}
			>
				<Command shouldFilter={false} loop>
					{/* Input row */}
					<div className="flex items-center gap-2 px-4 border-b border-[var(--color-border)]">
						{searching ? <SpinnerIcon /> : <SearchIcon semantic={isSemantic} />}
						<Command.Input
							autoFocus
							value={query}
							onValueChange={setQuery}
							placeholder={
								isSemantic
									? "Semantic search… (remove ~ for keyword)"
									: "Search meetings… (~query for semantic)"
							}
							className="flex-1 bg-transparent py-3.5 text-sm text-[var(--color-text)] placeholder:text-[var(--color-text-muted)] outline-none"
						/>
						<kbd className="text-[10px] text-[var(--color-text-muted)] border border-[var(--color-border)] rounded px-1.5 py-0.5 select-none">
							esc
						</kbd>
					</div>

					{/* Results list */}
					<Command.List className="max-h-72 overflow-y-auto">
						{searchQuery.length < 2 ? (
							<div className="py-10 text-center text-sm text-[var(--color-text-muted)]">
								<p>Start typing to search your meetings</p>
								<p className="text-xs mt-1.5 opacity-60">
									Prefix with{" "}
									<code className="bg-[var(--color-surface-raised)] px-1 rounded">~</code> for
									semantic search
								</p>
							</div>
						) : (
							<Command.Empty className="py-10 text-center text-sm text-[var(--color-text-muted)]">
								No results found.
							</Command.Empty>
						)}

						{results.length > 0 && (
							<Command.Group
								heading={isSemantic ? "Semantic matches" : "Keyword matches"}
								className="[&_[cmdk-group-heading]]:px-4 [&_[cmdk-group-heading]]:py-2 [&_[cmdk-group-heading]]:text-[10px] [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wider [&_[cmdk-group-heading]]:text-[var(--color-text-muted)]"
							>
								{results.map((r) => (
									<Command.Item
										key={r.meetingId}
										value={r.meetingId}
										onSelect={() => handleSelect(r.meetingId)}
										className="flex flex-col gap-0.5 px-4 py-3 cursor-pointer aria-selected:bg-[var(--color-surface-raised)] transition-colors"
									>
										<div className="flex items-center justify-between gap-2">
											<span className="text-sm font-medium text-[var(--color-text)] truncate">
												{r.title}
											</span>
											<span className="text-[10px] text-[var(--color-text-muted)] flex-shrink-0">
												{r.date}
											</span>
										</div>
										{/* Excerpt: FTS5 snippet with <mark> tags from our own SQLite DB — not user-controlled HTML */}
										<p
											className="text-xs text-[var(--color-text-muted)] line-clamp-2 [&_mark]:bg-[var(--color-accent)]/20 [&_mark]:text-[var(--color-accent)] [&_mark]:rounded-sm [&_mark]:px-0.5"
											// biome-ignore lint/security/noDangerouslySetInnerHtml: FTS5 snippet(<mark>) comes from our own SQLite DB
											dangerouslySetInnerHTML={{ __html: r.excerpt }}
										/>
									</Command.Item>
								))}
							</Command.Group>
						)}
					</Command.List>

					{/* Footer hints */}
					<div className="border-t border-[var(--color-border)] px-4 py-2 flex items-center gap-4 text-[10px] text-[var(--color-text-muted)] select-none">
						<span>↑↓ navigate</span>
						<span>↵ open</span>
						<span>esc close</span>
						{isSemantic && (
							<span className="ml-auto text-[var(--color-accent)] font-medium">
								Semantic mode (all-MiniLM-L6-v2)
							</span>
						)}
					</div>
				</Command>
			</div>
		</div>
	)
}

// ─── Icons ────────────────────────────────────────────────────────────────────

function SearchIcon({ semantic }: { semantic: boolean }) {
	return semantic ? (
		<svg
			aria-hidden="true"
			width="15"
			height="15"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth={2}
			className="text-[var(--color-accent)] flex-shrink-0"
		>
			<path
				strokeLinecap="round"
				strokeLinejoin="round"
				d="M9.663 17h4.673M12 3v1m6.364 1.636l-.707.707M21 12h-1M4 12H3m3.343-5.657l-.707-.707m2.828 9.9a5 5 0 117.072 0l-.548.547A3.374 3.374 0 0014 18.469V19a2 2 0 11-4 0v-.531c0-.895-.356-1.754-.988-2.386l-.548-.547z"
			/>
		</svg>
	) : (
		<svg
			aria-hidden="true"
			width="15"
			height="15"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth={2}
			className="text-[var(--color-text-muted)] flex-shrink-0"
		>
			<circle cx="11" cy="11" r="8" />
			<path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-4.35-4.35" />
		</svg>
	)
}

function SpinnerIcon() {
	return (
		<svg
			aria-hidden="true"
			width="15"
			height="15"
			viewBox="0 0 24 24"
			className="text-[var(--color-accent)] flex-shrink-0 animate-spin"
			fill="none"
		>
			<circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
			<path
				className="opacity-75"
				fill="currentColor"
				d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
			/>
		</svg>
	)
}
