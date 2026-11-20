# v4 build roadmap (agent run, 2026-11-20)

Merged to `main` in PR #38 (2026-11-20, all CI green). Plan: [`CROSS-PLATFORM.md`](./CROSS-PLATFORM.md). Design rules: [`DESIGN-LANGUAGE.md`](./DESIGN-LANGUAGE.md).


## Backlog for the next session (written 2026-11-20, after PR #38 merged)

Work on a new branch off `main`, one PR per item or small group. Order is a suggestion.

1. **Per-process system audio (macOS).** The Core Audio tap records *all* computer audio (music, videos). Tap only the detected call app's process; fall back to all audio when no app is known.
2. **Release setup (REL-10).** `release.yml` with `tauri-action` on tag push → draft GitHub Release: macOS `.dmg` (arm64 + x64 or universal), Windows NSIS/MSI (WebView2 bootstrapper), Linux AppImage/.deb/.rpm (bundle onnxruntime, declare webkit2gtk-4.1, libpulse, appindicator deps). macOS hardened runtime + entitlements (mic, `NSAudioCaptureUsageDescription`, Apple Events/Accessibility), sign bundled dylibs, notarize. Windows `signCommand` (Azure Trusted Signing or OV cert). `tauri-plugin-updater` + `latest.json` + "Check for updates" in Settings → General. Single version source; `tauri icon`. Owner must buy: Apple Developer ($99/yr), Windows signing; generate updater key (`tauri signer generate`). Build everything so it runs unsigned until secrets exist.
3. **New logo + favicon/app icon assets.** Design in Paper first (file in CLAUDE.md, new page), following DESIGN-LANGUAGE.md ("minutae •" wordmark, ink + on-air dot). Deliver: app icon master (1024², macOS rounded-rect with margins, Windows/Linux square), tray/menu-bar template icon (monochrome, 16/32 @1x/@2x, filled variant while recording), favicon/web icons for the UI + gallery, light/dark checks. Regenerate `apps/desktop/src-tauri/icons/*` with `tauri icon`; replace the placeholder tray image in `shell.rs`.
4. **Mac menu-bar panel.** Click the tray icon → non-activating NSPanel anchored under it (tauri-nspanel already used by HUD/prompt), closes on outside click. Contents: Record/Stop + timer + levels, "Listening for calls", Pause / Mark moment, last 3 meetings with status + Retry, dictation, Open Minutae. New route `#/tray`, design in Paper first, light/dark screenshots. macOS only; Windows/Linux keep the native menu. Est. 3–5 h, 500–800 LOC.
5. **Live captions** (`recording://caption` is in the contract but never emitted).
6. **Echo cancellation** when not using headphones (mic hears the speakers).
7. **macOS 13–14.5 system-audio fallback** (ScreenCaptureKit) and **Wayland** global-shortcut portal.
8. **Import old databases (MIG-10)** from Swift and Tauri v1 apps.
9. **Hand-test** on real hardware: call prompt with a real Zoom/Meet call, dictation into TextEdit/VS Code, speaker rename; first run on Windows and Linux.
10. **Design decision:** white text on the red live pill is 3.6:1 contrast.

New ideas, not yet scoped (ask the owner before building):
- Recording-consent helper ("let others know" message + note in onboarding).
- Remember voices across meetings (name Speaker 2 once → recognized later).
- Calendar integration (titles, attendees → speaker names, expected call start).
- Diagnostics: log files on disk + "Copy diagnostics" button.
- Pre-flight checks: low disk space warning; keep the machine awake while recording (verify current behaviour first).
- Send summaries to Notion / Slack / email (Obsidian already works via `.md` files).
- Optional encryption at rest / app lock.
- Phone app (designed in Paper, no stack chosen).

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
- macOS system-audio tap records **all** computer audio (music, videos), not just the call. Tap only the detected call app's process (Core Audio process taps support per-process capture).
