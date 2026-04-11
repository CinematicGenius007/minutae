import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { useEffect, useState } from "react"
import { toast } from "sonner"
import { getAllSettings, setSetting } from "@/lib/db/settings-repo"
import type { AppSettings } from "@/lib/settings"

interface LocalTranscriptionStatus {
	whisper_binary_path: string
	model_path: string
	binary_exists: boolean
	model_exists: boolean
}

interface WhisperModelDownloadResult {
	model_path: string
	source_url: string
}

interface WhisperDownloadProgress {
	model: string
	downloaded_bytes: number
	total_bytes: number | null
	percent: number | null
}

export function SettingsPage() {
	const [settings, setSettings] = useState<AppSettings | null>(null)
	const [saving, setSaving] = useState<string | null>(null)
	const [localStatus, setLocalStatus] = useState<LocalTranscriptionStatus | null>(null)
	const [downloadingModel, setDownloadingModel] = useState(false)
	const [downloadProgress, setDownloadProgress] = useState<WhisperDownloadProgress | null>(null)

	useEffect(() => {
		getAllSettings().then(setSettings)
	}, [])

	useEffect(() => {
		if (!settings || settings.transcriptionProvider !== "local") return

		invoke<LocalTranscriptionStatus>("get_local_transcription_status", {
			model: settings.whisperModel,
		})
			.then(setLocalStatus)
			.catch(() => setLocalStatus(null))
	}, [settings])

	useEffect(() => {
		if (!settings || settings.transcriptionProvider !== "local") return

		let isMounted = true
		let unlisten: (() => void) | null = null

		listen<WhisperDownloadProgress>("whisper:model-download-progress", (event) => {
			if (!isMounted) return
			if (event.payload.model === settings.whisperModel) {
				setDownloadProgress(event.payload)
			}
		})
			.then((fn) => {
				unlisten = fn
			})
			.catch(() => {})

		return () => {
			isMounted = false
			unlisten?.()
		}
	}, [settings])

	async function refreshLocalStatus(model: AppSettings["whisperModel"]) {
		try {
			const status = await invoke<LocalTranscriptionStatus>("get_local_transcription_status", {
				model,
			})
			setLocalStatus(status)
		} catch {
			setLocalStatus(null)
		}
	}

	async function downloadSelectedModel() {
		if (!settings) return
		setDownloadingModel(true)
		setDownloadProgress({
			model: settings.whisperModel,
			downloaded_bytes: 0,
			total_bytes: null,
			percent: null,
		})
		try {
			const result = await invoke<WhisperModelDownloadResult>("download_whisper_model", {
				model: settings.whisperModel,
			})
			await refreshLocalStatus(settings.whisperModel)
			toast.success("Whisper model downloaded", {
				description: result.model_path,
			})
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error)
			toast.error("Couldn't download Whisper model", {
				description: message,
			})
		} finally {
			setDownloadingModel(false)
		}
	}

	function formatBytes(bytes: number) {
		if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`
		return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
	}

	async function save<K extends keyof AppSettings>(key: K, value: AppSettings[K]) {
		setSaving(key)
		await setSetting(key, value)
		setSettings((s) => (s ? { ...s, [key]: value } : s))
		setSaving(null)
		toast.success("Saved")
	}

	if (!settings) {
		return (
			<div className="flex h-full items-center justify-center">
				<span className="text-sm text-[var(--color-text-muted)]">Loading…</span>
			</div>
		)
	}

	return (
		<div className="flex flex-col h-full">
			{/* Header */}
			<div className="px-6 pt-5 pb-4 border-b border-[var(--color-border)]">
				<h1 className="text-base font-semibold text-[var(--color-text)]">Settings</h1>
			</div>

			<div className="flex-1 overflow-y-auto px-6 py-5 space-y-8 max-w-xl">
				{/* ── General ── */}
				<Section title="General">
					<Toggle
						label="Auto-detect meetings"
						description="Show a prompt when your mic is picked up by another app."
						checked={settings.autoDetectMeetings}
						onChange={(v) => save("autoDetectMeetings", v)}
					/>
					<Toggle
						label="Launch at login"
						description="Start Minutae automatically when you log in."
						checked={settings.launchAtLogin}
						onChange={(v) => save("launchAtLogin", v)}
					/>
					<NumberField
						label="Prompt delay"
						description="Seconds of mic activity before showing the recording prompt."
						value={settings.showPromptDelay}
						min={1}
						max={30}
						unit="s"
						onChange={(v) => save("showPromptDelay", v)}
					/>
				</Section>

				{/* ── Transcription ── */}
				<Section title="Transcription">
					<RadioGroup
						name="transcription-provider"
						label="Provider"
						value={settings.transcriptionProvider}
						options={[
							{
								value: "local",
								label: "Local (whisper.cpp)",
								description: "Free, offline, private.",
							},
							{
								value: "whisper-api",
								label: "OpenAI Whisper API",
								description: "Faster, costs money.",
							},
						]}
						onChange={(v) =>
							save("transcriptionProvider", v as AppSettings["transcriptionProvider"])
						}
					/>
					{settings.transcriptionProvider === "local" && (
						<>
							<RadioGroup
								name="whisper-model"
								label="Model"
								value={settings.whisperModel}
								options={[
									{ value: "tiny", label: "Tiny", description: "~75MB · fastest, lower accuracy." },
									{
										value: "base",
										label: "Base",
										description: "~142MB · good balance. Recommended.",
									},
									{ value: "small", label: "Small", description: "~466MB · better accuracy." },
									{
										value: "medium",
										label: "Medium",
										description: "~1.5GB · best accuracy, slow.",
									},
								]}
								onChange={(v) => save("whisperModel", v as AppSettings["whisperModel"])}
							/>
							{localStatus && (
								<div className="rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-surface-raised)] px-3 py-2 text-xs space-y-1">
									<p className="text-[var(--color-text)] font-medium">Local whisper.cpp status</p>
									<p
										className={
											localStatus.binary_exists
												? "text-[var(--color-success)]"
												: "text-[var(--color-destructive)]"
										}
									>
										Binary: {localStatus.binary_exists ? "Found" : "Missing"}
									</p>
									<p
										className={
											localStatus.model_exists
												? "text-[var(--color-success)]"
												: "text-[var(--color-destructive)]"
										}
									>
										Model: {localStatus.model_exists ? "Found" : "Missing"}
									</p>
									{!localStatus.binary_exists && (
										<p className="text-[var(--color-text-muted)] break-all">
											Expected binary at: {localStatus.whisper_binary_path}
										</p>
									)}
									{!localStatus.model_exists && (
										<p className="text-[var(--color-text-muted)] break-all">
											Expected model at: {localStatus.model_path}
										</p>
									)}
									<div className="pt-2 flex items-center gap-2">
										<button
											type="button"
											onClick={downloadSelectedModel}
											disabled={downloadingModel}
											className="rounded-[var(--radius-sm)] bg-[var(--color-accent)] px-3 py-1.5 text-xs font-medium text-white hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed transition-opacity"
										>
											{downloadingModel ? "Downloading…" : "Download selected model"}
										</button>
										<a
											href="https://huggingface.co/ggerganov/whisper.cpp/tree/main"
											target="_blank"
											rel="noreferrer"
											className="text-[var(--color-accent)] hover:underline"
										>
											Manual models →
										</a>
									</div>
									{downloadingModel && downloadProgress && (
										<div className="pt-2 space-y-1">
											<div className="flex items-center justify-between text-[11px] text-[var(--color-text-muted)]">
												<span>
													Downloading {downloadProgress.model} model
													{downloadProgress.percent != null &&
														` · ${Math.floor(downloadProgress.percent)}%`}
												</span>
												<span>
													{formatBytes(downloadProgress.downloaded_bytes)}
													{downloadProgress.total_bytes != null &&
														` / ${formatBytes(downloadProgress.total_bytes)}`}
												</span>
											</div>
											<div className="h-2 rounded-full bg-[var(--color-border)] overflow-hidden">
												<div
													className="h-full bg-[var(--color-accent)] transition-[width] duration-150"
													style={{
														width:
															downloadProgress.percent != null
																? `${Math.max(4, downloadProgress.percent)}%`
																: "20%",
													}}
												/>
											</div>
										</div>
									)}
								</div>
							)}
						</>
					)}
					{settings.transcriptionProvider === "whisper-api" && (
						<ApiKeyField
							label="OpenAI API key"
							value={settings.openaiApiKey ?? ""}
							placeholder="sk-…"
							onChange={(v) => save("openaiApiKey", v || null)}
							isSaving={saving === "openaiApiKey"}
						/>
					)}
				</Section>

				{/* ── AI Summary ── */}
				<Section title="AI Summary">
					<RadioGroup
						name="summary-provider"
						label="Provider"
						value={settings.summaryProvider}
						options={[
							{
								value: "gemini",
								label: "Gemini Flash",
								description: "Fast and cheap. Recommended.",
							},
							{ value: "claude", label: "Claude Haiku", description: "Anthropic's fastest model." },
							{ value: "openai", label: "GPT-4o mini", description: "OpenAI's efficient model." },
						]}
						onChange={(v) => save("summaryProvider", v as AppSettings["summaryProvider"])}
					/>
					{settings.summaryProvider === "gemini" && (
						<ApiKeyField
							label="Gemini API key"
							value={settings.geminiApiKey ?? ""}
							placeholder="AIza…"
							helpUrl="https://aistudio.google.com/app/apikey"
							helpLabel="Get a key →"
							onChange={(v) => save("geminiApiKey", v || null)}
							isSaving={saving === "geminiApiKey"}
						/>
					)}
					{settings.summaryProvider === "claude" && (
						<ApiKeyField
							label="Anthropic API key"
							value={settings.anthropicApiKey ?? ""}
							placeholder="sk-ant-…"
							helpUrl="https://console.anthropic.com/settings/keys"
							helpLabel="Get a key →"
							onChange={(v) => save("anthropicApiKey", v || null)}
							isSaving={saving === "anthropicApiKey"}
						/>
					)}
					{settings.summaryProvider === "openai" && (
						<ApiKeyField
							label="OpenAI API key"
							value={settings.summaryOpenaiApiKey ?? ""}
							placeholder="sk-…"
							onChange={(v) => save("summaryOpenaiApiKey", v || null)}
							isSaving={saving === "summaryOpenaiApiKey"}
						/>
					)}
				</Section>

				{/* ── Storage ── */}
				<Section title="Storage">
					<div className="text-xs text-[var(--color-text-muted)] space-y-1">
						<p>Recordings, summaries, and the database are stored at:</p>
						<code className="block bg-[var(--color-surface-raised)] px-3 py-2 rounded-[var(--radius-sm)] text-[var(--color-text)] font-mono">
							~/Library/Minutae/
						</code>
					</div>
					<NumberField
						label="Auto-delete recordings after"
						description="Delete audio files older than this many days. Leave blank to keep forever."
						value={settings.maxRecordingAgeDays ?? 0}
						min={0}
						max={365}
						unit="days"
						nullable
						onChange={(v) => save("maxRecordingAgeDays", v === 0 ? null : v)}
					/>
				</Section>

				{/* ── About ── */}
				<Section title="About">
					<div className="flex items-center justify-between text-xs">
						<span className="text-[var(--color-text-muted)]">Version</span>
						<span className="text-[var(--color-text)] font-mono">0.1.0</span>
					</div>
					<div className="flex items-center justify-between text-xs">
						<span className="text-[var(--color-text-muted)]">License</span>
						<span className="text-[var(--color-text)]">MIT</span>
					</div>
				</Section>
			</div>
		</div>
	)
}

// ─── Sub-components ───────────────────────────────────────────────────────────

function Section({ title, children }: { title: string; children: React.ReactNode }) {
	return (
		<section className="space-y-4">
			<h2 className="text-xs font-semibold uppercase tracking-wider text-[var(--color-text-muted)]">
				{title}
			</h2>
			<div className="space-y-4">{children}</div>
		</section>
	)
}

function Toggle({
	label,
	description,
	checked,
	onChange,
}: {
	label: string
	description?: string
	checked: boolean
	onChange: (v: boolean) => void
}) {
	return (
		<label className="flex items-start justify-between gap-4 cursor-pointer">
			<div>
				<p className="text-sm text-[var(--color-text)]">{label}</p>
				{description && (
					<p className="text-xs text-[var(--color-text-muted)] mt-0.5">{description}</p>
				)}
			</div>
			<button
				type="button"
				role="switch"
				aria-checked={checked}
				onClick={() => onChange(!checked)}
				className={`relative flex-shrink-0 w-9 h-5 rounded-full transition-colors mt-0.5 ${
					checked ? "bg-[var(--color-accent)]" : "bg-[var(--color-border)]"
				}`}
			>
				<span
					className={`absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white shadow transition-transform ${
						checked ? "translate-x-4" : "translate-x-0"
					}`}
				/>
			</button>
		</label>
	)
}

function RadioGroup({
	name,
	label,
	value,
	options,
	onChange,
}: {
	name: string
	label: string
	value: string
	options: { value: string; label: string; description?: string }[]
	onChange: (v: string) => void
}) {
	return (
		<fieldset>
			<legend className="text-sm text-[var(--color-text)] mb-2">{label}</legend>
			<div className="space-y-2">
				{options.map((opt) => (
					<label
						key={opt.value}
						className={`flex items-start gap-3 p-3 rounded-[var(--radius-sm)] border cursor-pointer transition-colors ${
							value === opt.value
								? "border-[var(--color-accent)] bg-[var(--color-accent)]/5"
								: "border-[var(--color-border)] hover:border-[var(--color-text-muted)]"
						}`}
					>
						<input
							type="radio"
							name={name}
							value={opt.value}
							checked={value === opt.value}
							onChange={() => onChange(opt.value)}
							className="mt-0.5 accent-[var(--color-accent)]"
						/>
						<div>
							<p className="text-sm text-[var(--color-text)]">{opt.label}</p>
							{opt.description && (
								<p className="text-xs text-[var(--color-text-muted)] mt-0.5">{opt.description}</p>
							)}
						</div>
					</label>
				))}
			</div>
		</fieldset>
	)
}

function ApiKeyField({
	label,
	value,
	placeholder,
	helpUrl,
	helpLabel,
	onChange,
	isSaving,
}: {
	label: string
	value: string
	placeholder?: string
	helpUrl?: string
	helpLabel?: string
	onChange: (v: string) => void
	isSaving: boolean
}) {
	const [draft, setDraft] = useState(value)
	const [show, setShow] = useState(false)

	// Sync if parent value changes (e.g. on load)
	useEffect(() => {
		setDraft(value)
	}, [value])

	return (
		<div className="space-y-1.5">
			<div className="flex items-center justify-between">
				<span className="text-sm text-[var(--color-text)]">{label}</span>
				{helpUrl && (
					<a
						href={helpUrl}
						target="_blank"
						rel="noreferrer"
						className="text-xs text-[var(--color-accent)] hover:underline"
					>
						{helpLabel ?? "Get a key →"}
					</a>
				)}
			</div>
			<div className="flex gap-2">
				<div className="relative flex-1">
					<input
						type={show ? "text" : "password"}
						value={draft}
						placeholder={placeholder}
						onChange={(e) => setDraft(e.target.value)}
						className="w-full bg-[var(--color-surface-raised)] border border-[var(--color-border)] rounded-[var(--radius-sm)] px-3 py-2 text-sm text-[var(--color-text)] placeholder:text-[var(--color-text-muted)] font-mono focus:outline-none focus:border-[var(--color-accent)] pr-9"
					/>
					<button
						type="button"
						onClick={() => setShow((s) => !s)}
						className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[var(--color-text-muted)] hover:text-[var(--color-text)]"
					>
						{show ? <EyeOffIcon /> : <EyeIcon />}
					</button>
				</div>
				<button
					type="button"
					disabled={isSaving || draft === value}
					onClick={() => onChange(draft)}
					className="px-3 py-2 text-xs font-medium bg-[var(--color-accent)] text-white rounded-[var(--radius-sm)] hover:bg-[var(--color-accent-hover)] disabled:opacity-40 transition-colors"
				>
					{isSaving ? "Saving…" : "Save"}
				</button>
			</div>
		</div>
	)
}

function NumberField({
	label,
	description,
	value,
	min,
	max,
	unit,
	nullable,
	onChange,
}: {
	label: string
	description?: string
	value: number
	min: number
	max: number
	unit?: string
	nullable?: boolean
	onChange: (v: number) => void
}) {
	return (
		<div className="flex items-start justify-between gap-4">
			<div>
				<p className="text-sm text-[var(--color-text)]">{label}</p>
				{description && (
					<p className="text-xs text-[var(--color-text-muted)] mt-0.5">{description}</p>
				)}
			</div>
			<div className="flex items-center gap-1.5 flex-shrink-0">
				<input
					type="number"
					value={value === 0 && nullable ? "" : value}
					min={min}
					max={max}
					placeholder={nullable ? "∞" : undefined}
					onChange={(e) => onChange(e.target.value === "" ? 0 : Number(e.target.value))}
					className="w-16 text-right bg-[var(--color-surface-raised)] border border-[var(--color-border)] rounded-[var(--radius-sm)] px-2 py-1.5 text-sm text-[var(--color-text)] focus:outline-none focus:border-[var(--color-accent)]"
				/>
				{unit && <span className="text-xs text-[var(--color-text-muted)]">{unit}</span>}
			</div>
		</div>
	)
}

function EyeIcon() {
	return (
		<svg
			aria-hidden="true"
			width="14"
			height="14"
			fill="none"
			viewBox="0 0 24 24"
			stroke="currentColor"
			strokeWidth={2}
		>
			<path
				strokeLinecap="round"
				strokeLinejoin="round"
				d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z"
			/>
			<path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
		</svg>
	)
}

function EyeOffIcon() {
	return (
		<svg
			aria-hidden="true"
			width="14"
			height="14"
			fill="none"
			viewBox="0 0 24 24"
			stroke="currentColor"
			strokeWidth={2}
		>
			<path
				strokeLinecap="round"
				strokeLinejoin="round"
				d="M3.98 8.223A10.477 10.477 0 001.934 12C3.226 16.338 7.244 19.5 12 19.5c.993 0 1.953-.138 2.863-.395M6.228 6.228A10.45 10.45 0 0112 4.5c4.756 0 8.773 3.162 10.065 7.498a10.523 10.523 0 01-4.293 5.774M6.228 6.228L3 3m3.228 3.228l3.65 3.65m7.894 7.894L21 21m-3.228-3.228l-3.65-3.65m0 0a3 3 0 10-4.243-4.243m4.242 4.242L9.88 9.88"
			/>
		</svg>
	)
}
