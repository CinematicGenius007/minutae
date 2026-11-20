# Minutae — Claude Context

Local-first meeting recorder + dictation app for **macOS, Windows and Linux**. It notices when a call starts, records your mic and the other people (computer audio) as two tracks, transcribes locally with Whisper, tells speakers apart, summarizes with AI (Codex / Ollama / Grok / API keys), and offers system-wide dictation. Everything stays on the machine by default.

**Current version: v4 (Tauri 2 + Rust), branch `v4-tauri`.** The Swift app (`App/`, `Packages/`, `Minutae.xcodeproj`) and the old Tauri v1 app (`legacy/`) are kept only as reference until v4 reaches parity. Do not build new features there.

## Read before working

| Doc | What it is |
|---|---|
| `docs/DESIGN-LANGUAGE.md` | Design rules + CSS tokens. Wins over Paper if they disagree |
| `docs/CROSS-PLATFORM.md` | Architecture, per-OS native APIs, multi-speaker plan, perf targets, task list |
| `docs/IPC.md` | UI ↔ Rust contract (code in `apps/desktop/src/lib/{types,api,mock}.ts`) |
| `docs/REDESIGN.md` §1, §4, §5 | Decisions, screen list, behaviour (§2–3, §6–7 are superseded) |
| `docs/ROADMAP.md` | What was built, by whom, status |
| `docs/RELEASE.md` | Zero-cost release: self-signed macOS identity, Scoop, update channels |
| `docs/install/`, `docs/agents/INSTALL.md` | User install guides; the brief for agents installing Minutae for someone |

Design source (Paper, read-only for code work): https://app.paper.design/file/01M7W5R5J6SBFEN8K99EKT2NX0 — pages "Minutae — Redesign" (light) and "Minutae — Dark".

## Layout

```
apps/desktop/            Tauri 2 app
  src/                   React 19 + TS: shell/, screens/*, hooks/, lib/{types,api,mock}.ts
  src-tauri/             Rust: commands, dto, pipeline, recording, dictation, windows, tray
  e2e/                   Playwright screenshot + a11y tests (runs against mock.ts)
libs/ui/                 Component kit + tokens + fonts + gallery (NOT packages/ — clashes with Swift Packages/ on case-insensitive macOS)
crates/
  minutae-core           shared types, settings, per-OS paths
  minutae-store          SQLite (rusqlite), migrations, FTS5 search
  minutae-jobs           persistent job runner
  minutae-audio          mic + system audio capture (cpal; macOS process tap 14.6+, WASAPI loopback, Pulse monitor)
  minutae-detect         "another app is using the mic" per OS
  minutae-asr            whisper.cpp (whisper-rs), model manager
  minutae-diarize        sherpa-onnx diarization + two-track merge
  minutae-llm            transport router (codex-cli, grok-cli, ollama, api), summaries, ask
  minutae-dictation      hotkey state machine, post-processing, text insertion
  minutae-search         retrieval for "Ask"
```

## Commands

```bash
bun install                                   # JS workspaces (apps/*, libs/*)
bun run dev                                   # full app (tauri dev)
bun --cwd apps/desktop dev                    # UI only in a browser, fake backend (mock.ts); ?state=empty|recording|processing|failed
bun --cwd apps/desktop run test:e2e           # Playwright screenshots + axe (light/dark, chromium + webkit)
bun --cwd apps/desktop run test:e2e:update    # refresh screenshot baselines after an intended UI change
bun --cwd libs/ui dev                         # component gallery
cargo test --workspace                        # Rust (needs cmake: brew install cmake)
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p minutae-desktop --example e2e    # real record → transcribe → diarize → summarize on this machine
sh scripts/install.sh --from-source           # build + install like a release (macOS/Linux)
sh scripts/check-update.sh                    # installed vs latest release, prints the update command
```

CI: `.github/workflows/v4.yml` (macOS, Windows, Ubuntu). Windows uses the static CRT (`.cargo/config.toml`).

## Rules

1. UI uses only `libs/ui` components and tokens. No raw hex, px font sizes or ad-hoc spacing in screens.
2. The webview never does audio, AI or file work: it calls a command and listens for events.
3. Contract changes touch `types.ts`, `api.ts`, `mock.ts` and the Rust DTOs together.
4. Every Rust service has a trait + fake; tests never need a mic, model, CLI or permission.
5. Every screen ships empty / loading / error states in light and dark, with updated screenshot baselines.
6. Red (`--onair`) means "recording" and nothing else.
7. Never read or copy other apps' credentials; never pass skip-permission flags to CLIs.

## Data

macOS `~/Library/Minutae/`, Windows `%APPDATA%\Minutae\`, Linux `$XDG_DATA_HOME/minutae/` (override: `MINUTAE_DATA_DIR`). DB `minutae-v4.db`, `recordings/<meeting-id>/{mic,system}.wav`, `summaries/*.md` (YAML frontmatter), `models/`.
