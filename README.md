# Minutae

**Local-first meeting recorder and AI summarizer for Mac.**

Minutae detects when your mic goes live in Zoom, Meet, or Teams, records the audio, transcribes it offline with [whisper.cpp](https://github.com/ggerganov/whisper.cpp), and generates a structured summary with action items — all without sending your conversations to a server.

> **Status:** Early development. Not yet ready for general use.

---

## What it does

1. **Detects meetings automatically** — watches for mic activity from other apps via CoreAudio. When it spots a call starting, it asks if you want to record.
2. **Records locally** — captures mic audio as `.webm` using the Web MediaRecorder API. Files stay on your disk.
3. **Transcribes offline** — runs `whisper.cpp` locally (no API key, no internet required). Cloud Whisper API available as an opt-in fallback.
4. **Summarizes with AI** — sends the transcript to Gemini Flash by default. Claude and OpenAI are drop-in alternatives. The AI provider is a pluggable interface, not a hard dependency.
5. **Writes plain markdown** — every meeting produces a `.md` file with YAML frontmatter, a summary, key decisions, and an action item checklist. Your data is just files.
6. **Semantic search** — find past meetings by meaning using local ONNX embeddings (`all-MiniLM-L6-v2` via Transformers.js WASM). No cloud vector database.
7. **Tracks AI spend** — logs every API call (provider, model, tokens, estimated cost) and shows a usage dashboard.

---

## Design philosophy

- **Local-first.** Everything lives in `~/Library/Minutae/`. No account, no sync, no cloud required.
- **Open and hackable.** Summaries are plain markdown. Pipe them into Obsidian, version-control them, grep them.
- **Pluggable AI.** `TranscriptionProvider` and `SummaryProvider` are TypeScript interfaces. Swap providers in settings.
- **Offline capable.** whisper.cpp runs on-device. The app works with no internet connection.
- **Small.** Tauri binary is ~10MB. Not 120MB.

---

## Tech stack

| | |
|---|---|
| App shell | [Tauri 2](https://tauri.app) (Rust + WebView) |
| Frontend | React 19 + TypeScript |
| Styling | Tailwind CSS v4 |
| State | Zustand |
| Routing | TanStack Router |
| Database | SQLite + [Drizzle ORM](https://orm.drizzle.team) |
| Embeddings | [@xenova/transformers](https://github.com/xenova/transformers.js) (WASM, all-MiniLM-L6-v2) |
| Search | SQLite FTS5 (keyword) + JS cosine similarity (semantic) |
| Transcription | [whisper.cpp](https://github.com/ggerganov/whisper.cpp) (local) · Whisper API (optional) |
| Summarization | Gemini Flash · Claude · OpenAI (pluggable) |

---

## Output format

Each meeting writes a file to `~/Library/Minutae/summaries/`:

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
- Decided to push the redesign launch to May to allow time for performance review
- Agreed on a two-phase rollout: internal beta first, then public

## Key Decisions
- May launch date locked — no further slippage

## Action Items
- [ ] Alice to share updated mockups by Friday
- [ ] Bob to review the performance budget before end of week
- [ ] Carol to schedule user research sessions for April
```

---

## Installation

### Option A — Build from source (recommended, free, no signing needed)

This is the cleanest path. Building locally means macOS never quarantines the binary,
so Gatekeeper stays completely out of the way.

**Prerequisites:**
- [Rust](https://rustup.rs) (latest stable) — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Node.js ≥ 20 — [nodejs.org](https://nodejs.org) or `brew install node`
- pnpm — `npm i -g pnpm`
- Xcode Command Line Tools — `xcode-select --install`

```bash
git clone https://github.com/your-org/minutae
cd minutae
pnpm install
pnpm tauri build        # produces a .app + .dmg in src-tauri/target/release/bundle/
```

Drag `Minutae.app` to `/Applications`. Done. No warnings, no bypass needed.

> **First build is slow** (~5 min) while Rust compiles the Tauri runtime. Subsequent builds are fast.

---

### Option B — Download the unsigned DMG from GitHub Releases

Pre-built DMGs are attached to every [GitHub Release](https://github.com/your-org/minutae/releases).
Because the builds are not signed with an Apple Developer certificate ($99/yr), macOS will
block them by default. One command removes the quarantine flag:

```bash
# After dragging Minutae.app to /Applications:
xattr -cr /Applications/Minutae.app
```

Then open normally. You will not see this prompt again.

**Why does this happen?**
macOS adds a `com.apple.quarantine` extended attribute to anything downloaded from the internet.
Apps without an Apple Developer signature trigger Gatekeeper, which blocks launch.
`xattr -cr` strips the quarantine attribute — it is the standard workaround used by many
open-source Mac apps (e.g. older versions of VLC, Keka, etc.).

**Is it safe?**
Only if you trust this repository. Check that you downloaded from the official GitHub Releases
page and verify the SHA-256 checksum listed in the release notes before running `xattr`.

---

### Option C — Run in dev mode (no build needed)

```bash
git clone https://github.com/your-org/minutae
cd minutae
pnpm install
pnpm tauri dev
```

The app hot-reloads as you edit frontend code. Rust code requires a restart.

---

## Add an AI provider

Go to **Settings → AI Summary**, choose your provider, and paste an API key.

| Provider | Where to get a key |
|---|---|
| Gemini Flash (default, cheapest) | [aistudio.google.com](https://aistudio.google.com) |
| Claude | [console.anthropic.com](https://console.anthropic.com) |
| OpenAI | [platform.openai.com](https://platform.openai.com) |

For offline-only use (transcription without AI summaries), no API key is needed.

---

## Project structure

```
src/                  # React frontend
  components/         # UI components + layout shell
  pages/              # One file per route
  hooks/              # useRecorder, useSettings, etc.
  store/              # Zustand state
  lib/
    db/               # Drizzle schema + repository layer
    providers/        # TranscriptionProvider + SummaryProvider implementations
    prompts/          # AI prompt strings
    embeddings/       # Local ONNX embed() helper (Transformers.js)

src-tauri/            # Rust backend
  src/
    commands/         # Tauri commands (file I/O, recording save)
    audio/            # CoreAudio mic monitor
    db/               # SQLite init + migration runner
    models/           # whisper.cpp shell integration
  migrations/         # SQL migrations (run at startup via tauri-plugin-sql)
```

---

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for full setup and guidelines.

Quick checks before opening a PR:

```bash
pnpm typecheck         # TypeScript strict check
pnpm lint              # Biome lint
pnpm lint:fix          # Biome with auto-fix
cd src-tauri && cargo check
```

---

## Roadmap

See [PLAN.md](./PLAN.md) for the full implementation checklist.

**v0.1.0 target:** full pipeline working end to end (mic detect → record → transcribe → summarize → markdown file + search). Mac only.

**Later:**
- Speaker diarization
- Calendar integration (auto-title from Google/Apple Calendar)
- Notion / Obsidian / Logseq export
- Windows + Linux
- Real-time transcription
- Menu bar mode
- Auto-updater

---

## License

MIT
