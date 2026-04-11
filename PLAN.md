# Minutae — Detailed Implementation Plan

> Meeting recorder + AI summarizer. Local-first, open-source, Tauri-based.
> Stack: Tauri (Rust) · React · TypeScript · SQLite/Drizzle · sqlite-vss · whisper.cpp · Gemini Flash

---

## Phase 0 — Project Scaffolding

### 0.1 Repo & Tooling
- [ ] `create-tauri-app` with React + TypeScript template
- [ ] Configure `pnpm` as package manager (workspace-friendly, fast)
- [ ] Set up `biome` for linting + formatting (replaces ESLint + Prettier, faster)
- [ ] Add `husky` + `lint-staged` pre-commit hooks
- [ ] `.editorconfig` for consistent whitespace
- [ ] `tsconfig.json` — strict mode, path aliases (`@/` → `src/`)
- [ ] Add `.nvmrc` / `.node-version` pinning Node version

### 0.2 Tauri Project Structure
```
src/                    # React frontend
  components/
  pages/
  hooks/
  lib/                  # providers, utils
  store/                # Zustand state
src-tauri/
  src/
    commands/           # Tauri commands exposed to frontend
    audio/              # CoreAudio mic detection
    db/                 # SQLite + migrations runner
    models/             # whisper.cpp bindings
  Cargo.toml
  tauri.conf.json
```
- [ ] Scaffold folder structure above
- [ ] Configure Tauri `tauri.conf.json`: bundle identifier `com.nebula.minutae`, app name, icons
- [ ] Set up Tauri allowlist (permissions): filesystem, shell, notification, window, clipboard
- [ ] Add Tauri `updater` plugin skeleton (for later auto-updates)

### 0.3 CI/CD Skeleton
- [ ] GitHub Actions workflow: lint + typecheck on PRs
- [ ] GitHub Actions workflow: `tauri build` on push to `main` (Mac artifact)
- [ ] Add `CHANGELOG.md` + conventional commits convention

---

## Phase 1 — Data Layer

### 1.1 SQLite + Drizzle ORM
- [ ] Install `better-sqlite3` + `drizzle-orm` + `drizzle-kit`
- [ ] Define DB path: `~/Library/Minutae/minutae.db` (use Tauri `appDataDir`)
- [ ] Write `src-tauri` Rust side: open DB connection on app start, run migrations

**Schema — define in `src/lib/db/schema.ts`:**

```ts
// meetings
id, title, started_at, ended_at, duration_seconds, status
// recordings
id, meeting_id, file_path, file_size_bytes, created_at
// transcripts
id, meeting_id, raw_text, created_at, provider (whisper.cpp | whisper-api)
// segments (for word-level timestamps)
id, transcript_id, start_ms, end_ms, text, speaker_label
// summaries
id, meeting_id, markdown_content, model, created_at
// action_items
id, meeting_id, text, done, created_at
// embeddings
id, meeting_id, chunk_text, chunk_index, vector BLOB
// api_usage_log
id, meeting_id, provider, model, input_tokens, output_tokens,
   cost_usd, endpoint, created_at
// settings (key-value)
key, value, updated_at
```

- [ ] Write all Drizzle schema definitions
- [ ] Write initial migration file (`0001_init.sql`)
- [ ] Write migration runner (auto-applies on app start)
- [ ] Create Drizzle query helpers / repository layer:
  - `meetings.ts` — CRUD
  - `transcripts.ts` — CRUD + segment insert batch
  - `summaries.ts` — CRUD
  - `action_items.ts` — CRUD + toggle done
  - `api_usage.ts` — insert log, aggregate queries
  - `settings.ts` — get/set typed settings

### 1.2 File System Layout
- [ ] On first launch, create directory tree:
  ```
  ~/Library/Minutae/
    recordings/
    summaries/
    models/         ← whisper.cpp model files live here
    exports/
  ```
- [ ] Tauri Rust command: `ensure_app_dirs()` — called at startup
- [ ] Helper: `getRecordingPath(meetingId)` → `recordings/{date}-{id}.webm`
- [ ] Helper: `getSummaryPath(meeting)` → `summaries/YYYY-MM-DD-{slug}.md`

---

## Phase 2 — Mic Detection (Rust / CoreAudio)

### 2.1 CoreAudio Integration
- [ ] Add Rust crates: `coreaudio-rs` or raw `core-foundation` + `core-audio-types`
- [ ] Write Rust module `src-tauri/src/audio/mic_monitor.rs`:
  - Poll `kAudioDevicePropertyDeviceIsRunningSomewhere` every 2s
  - Detect when *any* process (not Minutae itself) activates the mic
  - Emit Tauri event `mic:activity-detected` to frontend
  - Emit `mic:activity-stopped` when mic goes idle
- [ ] Handle permission: check `AVCaptureDevice.authorizationStatus` for audio; if denied, surface settings deeplink
- [ ] Expose Tauri command `start_mic_monitor()` / `stop_mic_monitor()`
- [ ] Test: works with Zoom, Google Meet, Teams, FaceTime

### 2.2 Meeting Detection UX
- [ ] Frontend listens to `mic:activity-detected` event
- [ ] Show floating prompt (small, non-intrusive, top-right): "Looks like you're in a meeting — start recording?"
  - Buttons: **Start Recording** / **Dismiss** / **Don't ask again**
- [ ] If dismissed, re-prompt after 2 minutes of continued activity
- [ ] "Don't ask again" → stored in settings, suppresses prompts for that session
- [ ] Auto-dismiss prompt after 30s of no interaction

---

## Phase 3 — Recording

### 3.1 MediaRecorder (Frontend)
- [ ] Hook `useRecorder.ts`:
  - `start()` — `getUserMedia({ audio: true })` → `MediaRecorder` with `audio/webm;codecs=opus`
  - `pause()` / `resume()` — `MediaRecorder.pause()` / `resume()`
  - `stop()` → collect all `ondataavailable` chunks → `Blob` → pass to Tauri
  - Track elapsed duration with `useInterval`
  - Expose: `status`, `duration`, `audioLevel` (for VU meter)
- [ ] Audio level meter: use `AnalyserNode` from `AudioContext` — emit RMS values every 100ms
- [ ] Tauri command `save_recording(bytes: Vec<u8>, meeting_id: String) -> String` (returns path)
  - Write to `~/Library/Minutae/recordings/`
  - Insert row into `recordings` table
- [ ] Handle mic permission denial gracefully: explain why it's needed, show how to enable in System Settings

### 3.2 Recording State Machine
States: `idle → detecting → recording → paused → processing → done`
- [ ] Zustand store `useRecordingStore.ts`:
  - `currentMeeting: Meeting | null`
  - `status: RecordingStatus`
  - `startedAt: Date | null`
  - `actions`: `startMeeting`, `pauseRecording`, `resumeRecording`, `stopMeeting`
- [ ] On `startMeeting`: create DB row in `meetings` (status=`recording`), start `useRecorder`
- [ ] On `stopMeeting`: stop recorder, save file, update meeting `ended_at`, trigger pipeline

---

## Phase 4 — Transcription Pipeline

### 4.1 `TranscriptionProvider` Interface
```ts
// src/lib/providers/transcription.ts
interface TranscriptionProvider {
  name: string
  transcribe(audioPath: string, options?: TranscribeOptions): Promise<TranscriptionResult>
}

interface TranscriptionResult {
  text: string
  segments: Segment[]  // { start_ms, end_ms, text, speaker? }
  language: string
  duration_ms: number
}
```
- [ ] Define `TranscriptionProvider` + `TranscriptionResult` interfaces
- [ ] Factory function `getTranscriptionProvider(settings)` returns active impl

### 4.2 whisper.cpp (Local, Default)
- [ ] Add Tauri Rust command that shells to `whisper.cpp` binary bundled in app resources:
  - `src-tauri/resources/whisper` (compiled for Mac arm64 + x86_64 via universal binary)
  - Command: `transcribe_local(audio_path, model_name) -> TranscriptionResult`
  - Parse whisper JSON output (`--output-json` flag)
- [ ] Model management:
  - Default: `ggml-base.en.bin` (~142MB) — auto-downloaded on first use from Hugging Face
  - Settings: allow choosing `tiny`, `base`, `small`, `medium`
  - Show download progress in UI (Tauri progress event)
  - Store models in `~/Library/Minutae/models/`
- [ ] `WhisperLocalProvider` class implementing `TranscriptionProvider`
- [ ] Validate whisper binary runs on user's machine (test on first launch)

### 4.3 Whisper API (Cloud Fallback)
- [ ] `WhisperAPIProvider` class:
  - Uses `openai` npm package, hits `/v1/audio/transcriptions`
  - Handles file size limit (25MB) — chunk large files
  - Logs API call to `api_usage_log`
- [ ] Settings toggle: "Use cloud Whisper (faster, costs money)"

### 4.4 Transcription Job Queue
- [ ] Simple queue in `useTranscriptionStore.ts` (don't process two at once — CPU heavy)
- [ ] On `stopMeeting`: enqueue job `{ meetingId, audioPath }`
- [ ] Status updates: `queued → transcribing → done | failed`
- [ ] On completion: store transcript + segments in DB, trigger summarization
- [ ] Retry logic: up to 2 retries on failure, surface error in UI

---

## Phase 5 — AI Summary Pipeline

### 5.1 `SummaryProvider` Interface
```ts
interface SummaryProvider {
  name: string
  summarize(transcript: string, options?: SummaryOptions): Promise<SummaryResult>
}

interface SummaryResult {
  markdown: string          // full summary as markdown
  actionItems: string[]     // extracted checklist items
  title: string             // suggested meeting title
  keyDecisions: string[]
  participants: string[]    // names mentioned
}
```
- [ ] Define interfaces
- [ ] Factory `getSummaryProvider(settings)`

### 5.2 Gemini Flash Provider (Default)
- [ ] `GeminiProvider` class using `@google/generative-ai` SDK
- [ ] System prompt (in `src/lib/prompts/summarize.ts`):
  ```
  You are a meeting assistant. Given a transcript, produce:
  1. A concise summary (3-5 bullet points)
  2. Key decisions made
  3. Action items (as a markdown checklist)
  4. Suggested meeting title
  5. Names of participants mentioned
  Return as structured JSON.
  ```
- [ ] Parse response, validate schema with `zod`
- [ ] Log to `api_usage_log`: model, input_tokens, output_tokens, estimated cost
- [ ] Handle rate limits (retry with backoff), handle context length limits (chunk transcript)

### 5.3 Claude / OpenAI Providers (Pluggable)
- [ ] `ClaudeProvider` implementing `SummaryProvider` — `@anthropic-ai/sdk`, same prompt
- [ ] `OpenAIProvider` implementing `SummaryProvider` — `openai` sdk, same prompt
- [ ] Provider selection in settings — dropdown with API key fields per provider

### 5.4 Markdown Output
- [ ] After summarization, write `.md` file to `~/Library/Minutae/summaries/YYYY-MM-DD-{slug}.md`
- [ ] Frontmatter:
  ```yaml
  ---
  title: "Q2 Design Sync"
  date: 2026-04-11T14:30:00
  duration: 32m 14s
  participants: [Alice, Bob, Carol]
  tags: []
  recording: ../recordings/2026-04-11-abc123.webm
  ---
  ```
- [ ] Body: summary bullets → key decisions → action items checklist
- [ ] Tauri command `write_summary_file(path, content)` — uses Rust fs write

---

## Phase 6 — Semantic Search ✅

### 6.1 Embeddings (ONNX via Transformers.js)
- [x] `@xenova/transformers` (runs in WebView via WASM — no Node.js needed)
- [x] Model: `Xenova/all-MiniLM-L6-v2` — downloads from HuggingFace on first use, cached in browser Cache Storage
- [x] `src/lib/embeddings/embed.ts`:
  - `embed(text): Promise<Float32Array>` — lazy singleton pipeline
  - `chunkText(text, 200, 20)` — word-based chunks with overlap
  - `cosineSimilarity(a, b)` — in-memory vector comparison
- [x] `src/store/embeddings.ts` — Zustand job queue (enqueued after summary completes)
- [x] `src/lib/db/embeddings.ts` — `storeEmbeddings`, FTS5 + semantic search functions
- [x] Migration `0002_fts_triggers.sql` — keeps `transcripts_fts` in sync via INSERT/UPDATE/DELETE triggers
- _Note: sqlite-vss deferred — JS cosine similarity is sufficient for v0.1.0 scale_

### 6.2 Search UI
- [x] `cmdk` library for the Command palette
- [x] Global `Cmd+K` shortcut (keyboard listener in `RootLayout`)
- [x] Search button in sidebar with `⌘K` hint badge
- [x] Two modes:
  - **Keyword** — SQLite FTS5 with `snippet()` highlight (`<mark>` tags)
  - **Semantic** — prefix query with `~` → embed → cosine similarity over all stored chunks
- [x] Results: meeting title, date, highlighted excerpt
- [x] Click result → navigate to `/meetings/$id`

---

## Phase 7 — UI ✅

### 7.1 Design System
- [x] CSS custom property design tokens (dark mode, `--color-*` variables in `index.css`)
- [x] Tailwind CSS v4 with `@theme {}` integration
- [x] Color tokens: `--color-bg: #0e0e10`, `--color-accent: #7c6df0`, `--color-success: #3ecf8e`, etc.
- [x] Typography: `Geist` font (variable)
- _Note: `shadcn/ui` and `framer-motion` deferred — not needed for v0.1.0_

### 7.2 App Shell
- [x] Titlebar: custom (`decorations: false`, `titleBarStyle: Overlay`), traffic lights reserved with `pl-20`
- [x] Sidebar (240px fixed): logo, nav links (Meetings, Usage, Settings), mic status indicator
- [x] Main area: TanStack Router (`/`, `/meetings/$id`, `/usage`, `/settings`)
- [x] Global floating recording pill: VU meter, MM:SS timer, pause/resume/stop
- [x] Global meeting prompt overlay: shown 2s after mic activity detected, auto-dismisses at 30s

### 7.3 Pages

**Meetings List (`/`)** ✅
- [x] List of past meetings, sorted by date desc
- [x] Each row: title, date, duration, status badge, colored stripe
- [x] Empty state with mic icon

**Meeting Detail (`/meetings/:id`)** ✅
- [x] Header: title (editable inline), date, duration
- [x] Tabs: **Summary** | **Transcript** | **Action Items** | **Recording**
- [x] Summary tab: rendered markdown (`react-markdown` + `remark-gfm`)
- [x] Transcript tab: segments with timestamps
- [x] Action Items tab: pending/done sections, toggleable checklist
- [x] Recording tab: placeholder (Tauri asset protocol wired in later phase)

**Recording Overlay (RecordingPill)** ✅
- [x] Live VU meter (8 animated bars)
- [x] Timer: `MM:SS`
- [x] Pause / Resume / Stop buttons
- [x] Status label: "Recording" / "Paused"

**Settings (`/settings`)** ✅
- [x] Sections: General | Transcription | AI Summary | Storage | About
- [x] All settings persisted live to SQLite with Sonner toast confirmation
- [x] API key fields: masked input, show/hide toggle, deferred Save button

**Usage Dashboard (`/usage`)** ✅
- [x] 4 stat cards: Total spend, This month, Meetings processed, Hours transcribed
- [x] API usage log table: date, provider badge, model, tokens in/out, cost
- _Note: recharts charts deferred to v0.2.0_

### 7.4 Notifications
- [x] In-app toast system (`sonner`) for settings saves, errors, completion events
- [ ] Tauri OS notification: "Meeting recorded — summary ready" (Phase 9)
- [ ] Menu bar icon: recording status (Phase 11/future)

---

## Phase 8 — Settings & Config

### 8.1 Settings Schema (Typed)
```ts
interface AppSettings {
  // General
  launchAtLogin: boolean
  autoDetectMeetings: boolean
  showPromptDelay: number           // seconds before prompt
  // Transcription
  transcriptionProvider: 'local' | 'whisper-api'
  whisperModel: 'tiny' | 'base' | 'small' | 'medium'
  openaiApiKey: string | null
  // Summary
  summaryProvider: 'gemini' | 'claude' | 'openai'
  geminiApiKey: string | null
  anthropicApiKey: string | null
  summaryOpenaiApiKey: string | null
  // Storage
  recordingsPath: string            // default ~/Library/Minutae/recordings
  summariesPath: string
  maxRecordingAgeDays: number | null  // null = keep forever
}
```
- [ ] Define `AppSettings` zod schema
- [ ] `useSettings` hook: reads from DB `settings` table, writes on change
- [ ] Encrypt API keys at rest using Tauri's `stronghold` or OS keychain (`security` on Mac)
- [ ] `launchAtLogin`: use `tauri-plugin-autostart`

---

## Phase 9 — Packaging & Distribution ✅ (config complete)

### 9.1 Mac Build
- [x] Tauri bundle targets: `aarch64-apple-darwin` + `x86_64-apple-darwin` (matrix in CI)
- [x] DMG installer configured in `tauri.conf.json` (window size, app/folder positions)
- [x] `src-tauri/entitlements.plist` — microphone, network, file access, JIT (for WASM)
- [x] `tauri.conf.json` bundle metadata: category, copyright, descriptions
- [ ] Code signing: set `APPLE_SIGNING_IDENTITY` in repo secrets when publishing
- [ ] Notarization: set `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` secrets
- [ ] `.app` icon set: `pnpm tauri icon path/to/icon.png` to regenerate all sizes

### 9.2 Release Process
- [x] `.github/workflows/release.yml` — triggers on `v*` tags, builds both arches, creates GitHub Release draft
- [x] `tauri-apps/tauri-action@v0` handles build + artifact upload
- [ ] Auto-updater (`tauri-plugin-updater`) — Phase 11 / post v0.1.0

### 9.3 Open Source Prep
- [x] `README.md` — pitch, features, dev setup, tech stack, roadmap
- [ ] `LICENSE` — MIT (add before first public release)
- [ ] `CONTRIBUTING.md` — dev setup, PR guidelines
- [ ] `.github/ISSUE_TEMPLATE/` — bug report + feature request templates

---

## Phase 10 — Testing

### 10.1 Unit Tests
- [ ] `vitest` for frontend unit tests
- [ ] Test: `TranscriptionProvider` interface implementations (mock whisper output)
- [ ] Test: `SummaryProvider` implementations (mock API responses)
- [ ] Test: Drizzle repository layer (in-memory SQLite)
- [ ] Test: Markdown file generation (snapshot tests)
- [ ] Test: Settings schema validation (zod)

### 10.2 Integration Tests
- [ ] Rust: `cargo test` for audio monitor, file system commands, DB init
- [ ] End-to-end: `tauri-driver` + `WebdriverIO` — test full record → transcribe → summarize flow (mocked APIs)

### 10.3 Manual Test Checklist (before each release)
- [ ] Mic detection fires when Zoom/Meet/Teams starts
- [ ] Recording starts, pauses, resumes, stops correctly
- [ ] `.webm` file written to correct path
- [ ] Local whisper transcription completes (base.en model)
- [ ] Gemini summarization completes, action items extracted
- [ ] Markdown file written with correct frontmatter
- [ ] Semantic search returns relevant results
- [ ] Usage dashboard shows correct cost estimate
- [ ] Settings persist across app restart
- [ ] App works fully offline (local whisper + no summary provider needed)

---

## Phase 11 — Future / Backlog (not in v1)

- [ ] Speaker diarization (pyannote.audio via Python sidecar)
- [ ] Calendar integration (detect meetings from Google Calendar / Apple Calendar)
- [ ] Notion / Obsidian / Logseq export
- [ ] Menu bar app mode (always visible mic indicator)
- [ ] Windows / Linux builds
- [ ] Real-time transcription during recording (streaming whisper)
- [ ] Team sync (shared meetings via self-hosted server)
- [ ] AI-powered follow-up email draft

---

## Implementation Order (Recommended)

```
Phase 0 (Scaffold) → Phase 1 (DB) → Phase 2 (Mic detection) →
Phase 3 (Recording) → Phase 4 (Transcription) → Phase 5 (Summaries) →
Phase 7.1-7.3 (UI — build as features land) → Phase 6 (Search) →
Phase 8 (Settings) → Phase 9 (Packaging) → Phase 10 (Tests)
```

**First working demo target:** Phases 0–5 complete = detect mic → record → transcribe → summarize → write markdown. No pretty UI needed, just the pipeline working end to end.

**v0.1.0 release target:** All phases 0–9 complete, minus Phase 6 (search can be v0.2.0).
