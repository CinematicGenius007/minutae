# v4 build roadmap (agent run, 2026-11-20)

Branch `v4-tauri`. Plan: [`CROSS-PLATFORM.md`](./CROSS-PLATFORM.md). Design rules: [`DESIGN-LANGUAGE.md`](./DESIGN-LANGUAGE.md).

| Phase | Work | Who | Folders owned |
|---|---|---|---|
| 0 | Workspace scaffold, shared types (`minutae-core`), Tauri shell | lead | root, `crates/minutae-core`, `apps/desktop` skeleton |
| 1a | Component kit + gallery, light/dark | agent `ui-kit` | `libs/ui` |
| 1b | Audio capture (mic + system, per OS) + call detection | agent `audio` | `crates/minutae-audio`, `crates/minutae-detect` |
| 1c | Whisper transcription + speaker diarization + track merge | agent `asr` | `crates/minutae-asr`, `crates/minutae-diarize` |
| 1d | SQLite store, job runner, search | agent `store` | `crates/minutae-store`, `crates/minutae-jobs`, `crates/minutae-search` |
| 1e | LLM router port + summaries + ask | agent `llm` | `crates/minutae-llm` |
| 1f | Dictation engine + text insertion | agent `dictation` | `crates/minutae-dictation` |
| 2 | IPC contract (`docs/IPC.md`) | lead | `docs/IPC.md` |
| 2a | Tauri backend: state, commands, events, pipeline, tray, windows, plugins | agent `backend` | `apps/desktop/src-tauri` |
| 2b | Screens: shell, Meetings, Live, Detail | agent `screens-1` | `apps/desktop/src/screens/{meetings,live,detail}`, shell |
| 2c | Screens: Dictate, Library, Settings ×6, Onboarding, HUD, Prompt | agent `screens-2` | `apps/desktop/src/screens/{dictate,library,settings,onboarding,overlays}` |
| 3 | Build + test on macOS, CI for all 3 OS, screenshot checks, review, fixes | agent `qa` + lead | all |

Rules for every agent: edit only owned folders; add Rust deps only in your crate's `Cargo.toml`; own `CARGO_TARGET_DIR`; no commits; tests use fakes; report public API + what is verified vs stubbed.

Known limit of this run: only macOS hardware is available. Windows and Linux code paths are compiled behind `cfg` and checked in CI, not run by hand.

## Status (end of run, 2026-11-20)

All phases complete on branch `v4-tauri` (uncommitted).

| Check | Result |
|---|---|
| `cargo test --workspace` | 208 passed, 0 failed (3 real-model tests ignored by default; pass when run) |
| `cargo clippy --workspace --all-targets -D warnings` | clean |
| UI kit `bun test` + typecheck | 16 passed |
| App typecheck + `vite build` | pass |
| Playwright (`test:e2e`) | 171 passed, 1 skipped: 108 screenshots (every screen × state × light/dark, chromium + webkit), axe a11y on 14 routes × 2 themes |
| Real pipeline on this Mac (`examples/e2e`) | record 8 s mic + system → Whisper → 3 speakers → Codex summary → meeting Done |
| `tauri build --debug` | `Minutae.app` builds and launches cleanly; quits cleanly |

Not verified here (no hardware): Windows and Linux runtime. They compile in the CI workflow, which has not run yet.

Open items:
- Run `v4.yml` on GitHub (needs `Cargo.lock` committed); fix whatever Windows/Linux surface.
- Hand-test in the running app: call prompt with a real Zoom/Meet call, dictation typing into TextEdit/VS Code, speaker renaming.
- Live captions during recording (`recording://caption` not emitted yet).
- Echo cancellation when not wearing headphones (mic picks up the speakers).
- macOS 13–14.5 system-audio fallback (ScreenCaptureKit); Wayland global shortcut portal.
- Import old (Tauri v1 / Swift) databases (MIG-10); signing + updater (REL-10).
- Design decision: white text on the red live pill is 3.6:1 contrast.
