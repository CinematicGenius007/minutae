# Minutae — Claude Context

This file gives Claude full context on the project so every session starts with a complete picture.

---

## What is Minutae?

Minutae is a **local-first, open-source desktop app** that automatically detects when you're in a meeting (via mic activity), records audio, transcribes it locally with whisper.cpp, and generates an AI summary with extracted action items. Everything stays on your machine by default — no cloud, no account required.

The name is a deliberate double meaning: "meeting minutes" + the small details that matter.

---

## Core Principles

1. **Local-first.** Audio, transcripts, summaries, and the database all live in `~/Library/Minutae/`. Your data is yours.
2. **Open and hackable.** Summaries are plain `.md` files with YAML frontmatter. Power users can pipe them into Obsidian, grep them, version-control them.
3. **AI pipeline is an interface.** `TranscriptionProvider` and `SummaryProvider` are TypeScript interfaces. Swapping Gemini for Claude is a config change, not a code change.
4. **Offline capable.** whisper.cpp runs entirely locally. The app works with zero internet.
5. **Small binary.** Tauri (~10MB) not Electron (~120MB).

---

## Tech Stack

| Layer | Choice | Why |
|---|---|---|
| App shell | Tauri 2 (Rust + WebView) | Small binary, native Mac APIs, proper `.app` bundle |
| Frontend | React 19 + TypeScript | Familiar, fast, good ecosystem |
| Styling | Tailwind CSS v4 + shadcn/ui | Dark-first, utility-first, no design debt |
| State | Zustand | Minimal, no boilerplate |
| Routing | TanStack Router | Type-safe, file-based-ready |
| DB | SQLite + Drizzle ORM | Local file, Drizzle supports Postgres later with same syntax |
| Vector search | sqlite-vss + ONNX (all-MiniLM-L6-v2) | Local embeddings, no cloud vector DB |
| Transcription | whisper.cpp (local) or Whisper API (cloud) | Free offline-first, cloud as fallback |
| Summarization | Gemini Flash (default), Claude, OpenAI (pluggable) | Fast + cheap default, easy to swap |
| Linting | Biome | Replaces ESLint + Prettier, much faster |
| Package manager | pnpm | Fast, disk-efficient |

---

## Project Structure

```
minutae/
├── src/                        # React frontend
│   ├── components/
│   │   ├── layout/             # RootLayout (sidebar + main shell)
│   │   └── ui/                 # shadcn/ui primitives
│   ├── pages/                  # One file per route
│   │   ├── MeetingsPage.tsx
│   │   ├── MeetingDetailPage.tsx
│   │   ├── UsagePage.tsx
│   │   └── SettingsPage.tsx
│   ├── hooks/                  # useRecorder, useSettings, etc.
│   ├── store/                  # Zustand stores
│   ├── lib/
│   │   ├── router.ts           # TanStack Router definition
│   │   ├── db/                 # Drizzle schema + repo layer
│   │   ├── providers/          # TranscriptionProvider, SummaryProvider impls
│   │   ├── prompts/            # AI prompt strings
│   │   └── embeddings/         # ONNX embed() helper
│   ├── App.tsx                 # RouterProvider entry
│   ├── main.tsx                # ReactDOM.createRoot
│   └── index.css               # Tailwind v4 + CSS custom properties (theme tokens)
│
├── src-tauri/
│   ├── src/
│   │   ├── main.rs             # Binary entry point
│   │   ├── lib.rs              # Tauri builder, plugin registration, setup
│   │   ├── commands/
│   │   │   └── fs.rs           # ensure_app_dirs, save_recording, write_summary_file
│   │   ├── audio/
│   │   │   └── mic_monitor.rs  # CoreAudio mic activity detection (Phase 2)
│   │   ├── db/
│   │   │   └── init.rs         # SQLite open + migration runner (Phase 1)
│   │   └── models/
│   │       └── whisper.rs      # whisper.cpp shell command (Phase 4)
│   ├── capabilities/           # Tauri permission files
│   ├── icons/
│   └── tauri.conf.json         # App config: com.nebula.minutae, 1200×800, titlebar overlay
│
├── PLAN.md                     # Full 11-phase implementation checklist
├── CLAUDE.md                   # This file
├── README.md                   # Public-facing documentation
├── biome.json                  # Linting + formatting config (CSS excluded — Tailwind v4)
├── tsconfig.json               # Strict mode, @/ path alias → src/
└── vite.config.ts              # Tailwind vite plugin, @/ alias, Tauri dev settings
```

---

## Data Flow (end to end)

```
Mic activity detected (CoreAudio, Rust)
    ↓ Tauri event: mic:activity-detected
Frontend prompt: "Start recording?"
    ↓ user clicks Start
useRecordingStore.startMeeting()
    → INSERT into meetings (status=recording)
    → useRecorder.start() — MediaRecorder(audio/webm)
        ↓ user clicks Stop
useRecordingStore.stopMeeting()
    → save .webm via Tauri save_recording command
    → UPDATE meetings (ended_at, status=processing)
    → enqueue transcription job
        ↓ whisper.cpp (local) or Whisper API
INSERT transcript + segments into DB
    → enqueue summarization job
        ↓ Gemini Flash (or Claude / OpenAI)
INSERT summary + action_items into DB
    → write .md file to ~/Library/Minutae/summaries/
    → UPDATE meetings (status=done)
    → Tauri notification: "Summary ready"
    → embed transcript chunks → store in embeddings table (sqlite-vss)
```

---

## Database Schema (SQLite via Drizzle)

Located at: `~/Library/Minutae/minutae.db`

| Table | Purpose |
|---|---|
| `meetings` | One row per session: title, started_at, ended_at, duration, status |
| `recordings` | File path + size for each .webm |
| `transcripts` | Raw text + provider used |
| `segments` | Word-level timestamps (start_ms, end_ms, text, speaker_label) |
| `summaries` | Markdown content, model used |
| `action_items` | Extracted checklist items, toggleable done state |
| `embeddings` | Chunk text + vector BLOB for semantic search |
| `api_usage_log` | Every AI API call: provider, model, tokens in/out, cost_usd |
| `settings` | Key-value store for app config |

---

## AI Provider Interfaces

Both are defined in `src/lib/providers/`:

```ts
interface TranscriptionProvider {
  name: string
  transcribe(audioPath: string, options?: TranscribeOptions): Promise<TranscriptionResult>
}

interface SummaryProvider {
  name: string
  summarize(transcript: string, options?: SummaryOptions): Promise<SummaryResult>
}
```

Implementations:
- **Transcription:** `WhisperLocalProvider` (default), `WhisperAPIProvider`
- **Summary:** `GeminiProvider` (default), `ClaudeProvider`, `OpenAIProvider`

Selected via settings; instantiated by factory functions `getTranscriptionProvider()` and `getSummaryProvider()`.

---

## File Output Format

Each meeting writes a markdown file to `~/Library/Minutae/summaries/YYYY-MM-DD-{slug}.md`:

```markdown
---
title: "Q2 Design Sync"
date: 2026-04-11T14:30:00
duration: 32m 14s
participants: [Alice, Bob, Carol]
tags: []
recording: ../recordings/2026-04-11-abc123.webm
---

## Summary
- Decided to push the redesign launch to May
- ...

## Key Decisions
- ...

## Action Items
- [ ] Alice to share updated mockups by Friday
- [ ] Bob to review performance budget
```

---

## Settings Schema

Stored in the `settings` DB table (key-value), typed via Zod:

```ts
interface AppSettings {
  launchAtLogin: boolean
  autoDetectMeetings: boolean
  showPromptDelay: number           // seconds before showing the "start recording?" prompt
  transcriptionProvider: 'local' | 'whisper-api'
  whisperModel: 'tiny' | 'base' | 'small' | 'medium'
  openaiApiKey: string | null       // for Whisper API
  summaryProvider: 'gemini' | 'claude' | 'openai'
  geminiApiKey: string | null
  anthropicApiKey: string | null
  summaryOpenaiApiKey: string | null
  recordingsPath: string
  summariesPath: string
  maxRecordingAgeDays: number | null
}
```

API keys are encrypted at rest via OS keychain (Tauri stronghold or macOS `security`).

---

## Design Tokens

Defined in `src/index.css` as CSS custom properties:

```css
--color-bg: #0e0e10          /* app background */
--color-surface: #1a1a1e     /* cards, sidebar */
--color-surface-raised: #242428
--color-border: #2e2e34
--color-text: #e8e8ed
--color-text-muted: #8e8e99
--color-accent: #7c6df0      /* primary action color */
--color-destructive: #f04f4f
--color-success: #3ecf8e
```

Dark mode only. Font: Geist (variable).

---

## Dev Commands

```bash
pnpm dev              # Vite dev server only (frontend preview)
pnpm tauri dev        # Full Tauri app in dev mode (use this)
pnpm build            # Frontend production build
pnpm typecheck        # tsc --noEmit
pnpm lint             # Biome check
pnpm lint:fix         # Biome check --write (auto-fix)
pnpm format           # Biome format --write
```

---

## Implementation Status

See `PLAN.md` for the full 11-phase checklist. Current status:

- [x] **Phase 0** — Scaffolding complete (Tauri + React + TypeScript, Biome, Tailwind v4, TanStack Router, Zustand, folder structure, husky)
- [x] **Phase 1** — Data layer (SQLite schema, Drizzle, migrations, all 9 tables)
- [x] **Phase 2** — Mic detection (CoreAudio `kAudioDevicePropertyDeviceIsRunningSomewhere`, 2s poll, Tauri events)
- [x] **Phase 3** — Recording (MediaRecorder + webm/opus, VU meter, save via Tauri fs command)
- [x] **Phase 4** — Transcription (whisper.cpp shell command, WhisperAPIProvider fallback, sequential job queue)
- [x] **Phase 5** — AI summaries (GeminiProvider default, ClaudeProvider + OpenAIProvider, Zod validation, .md file output, action items extracted)
- [x] **Phase 6** — Semantic search (@xenova/transformers WASM, all-MiniLM-L6-v2, FTS5 triggers, cosine similarity in JS, Cmd+K SearchModal with cmdk)
- [x] **Phase 7** — Full UI (MeetingsPage, MeetingDetailPage, SettingsPage, UsagePage, RecordingPill, MeetingPrompt, Sonner toasts)
- [ ] **Phase 8** — Settings + API key management (OS keychain encryption)
- [x] **Phase 9** — Packaging + distribution (DMG config, entitlements.plist, GitHub Actions release workflow — signing/notarization needs Apple Developer secrets)
- [ ] **Phase 10** — Testing

**First milestone:** Phases 1–5 complete = mic detect → record → transcribe → summarize → `.md` file written end to end.

---

## Key Decisions & Rationale

| Decision | Rationale |
|---|---|
| Tauri over Electron | 10MB vs 120MB binary, native WebView, proper Mac `.app`, Rust for system APIs |
| whisper.cpp local default | Free, offline, private — no API key needed for basic use |
| Gemini Flash for summaries | Fast + cheap. Pluggable so power users can switch to Claude/OpenAI |
| sqlite-vss for vector search | No external server, no cost, works offline — keeps the local-first promise |
| Drizzle ORM | Postgres-compatible syntax means future cloud sync is feasible |
| Markdown file output | Hackable, readable, works with Obsidian/Logseq/any text tool |
| Biome over ESLint+Prettier | Single tool, 10-100x faster, same quality |
| CSS excluded from Biome | Tailwind v4 `@theme {}` syntax not yet supported by Biome's CSS parser |
