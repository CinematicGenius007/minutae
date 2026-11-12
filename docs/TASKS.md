# Minutae v3 (native Swift) — Implementation Task List

For the implementing agent. Read [`ARCHITECTURE.md`](./ARCHITECTURE.md) first, then `docs/llm-providers.md` and `docs/spike-01-results.md`. Design source of truth: Paper file "Xenial horizon", page "Ink & signal".

Written 2026-11-13. Replaces the Tauri task list (the old FND-01 to FND-06, LLM-02 to LLM-09, and the Rust and React lanes are obsolete; IDs below are new).

## Ownership

| Work | Owner |
|---|---|
| UI design (Paper), architecture, review | **Owner and design/architecture agent. Not yours.** |
| Implementation of everything below | **Implementing agent** |

- `DS-` tasks are design work. Do not do them. Do not build UI for a screen until its row in the Design status table says **approved**. Until then build the service and tests only.
- If a design or architecture decision is missing, stop and write the question in the PR description. Do not invent it.
- One task ID per PR. PRs are reviewed against the acceptance criteria.

Locked decisions (do not reopen): native Swift 6, macOS 14.4+; transports `codex-cli`, `grok-cli`, `ollama`, optional `api`; hold ⌥Space; keep dictation audio; every dictation logged; light theme only; not sandboxed; no paid Apple Developer account (free Apple Development certificate, no notarisation).

### Design status (maintained by the design agent)

| ID | Screen / topic | Status |
|---|---|---|
| Screens 01–13 | Meetings, Audio, Detected, Recording, Summary, Transcript, Dictate, Search, Settings, Usage, Onboarding, Live notes, Menu bar | approved (DS-01 soft popups and DS-02 menu-bar icon done) |
| DS-03 | Settings and Usage for Codex, Grok, Ollama, API; dictation settings | queued |
| DS-04 | Library, dictation detail, transcript editor, export, storage, HUD states, dictionary | queued |
| DS-05 | Swift component sheet: tokens, type scale, states for every control | queued |

## How to use this list

- Size: **S** ≤ half a day · **M** ~1 day · **L** 2–3 days · **XL** split first.
- Done means: acceptance criteria met, `xcodebuild test` green, CI green, no warnings in Swift 6 strict mode, SwiftLint clean.
- Every service has a protocol and a fake so tests never need real permissions, CLIs or models.
- Never read, copy or export credentials of any external CLI or app. Never call the `claude` shell function. Never pass skip-permission flags.
- Do not delete the Tauri app until `MIG-02`.

## Milestones

| M | Name | Goal |
|---|---|---|
| M0 | Foundation | project builds, tests run, DB opens the existing data, design system exists |
| M1 | AI without keys | Codex, Grok, Ollama and API transports behind one router |
| M2 | Dictation | hold ⌥Space, speak, text appears, logged |
| M3 | Meetings | record, transcribe, summarise, durable jobs, parity with the Tauri app |
| M4 | Library | unified search, editor, export, retention |
| M5 | UI | all approved screens ported |
| M6 | Release | signed, notarised, updater, migration, tag Tauri final |

---

## M0 — Foundation

### HK-01 · Close out the research PRs · S
Merge or close PR #2 (LLM-01) and PR #3 (SPIKE-01) as the owner directs. Keep `RESULTS.md` and `llm-providers.md` under `docs/`. Spike code stays on its branch, not in the product tree.
**Acceptance:** `main` has the two docs, no spike code outside `spikes/` or a branch.

### SW-01 · Project scaffold and CI · M
Create `Minutae.xcodeproj` (macOS 14.4 deployment target, Swift 6, hardened runtime) and the local packages in ARCHITECTURE §4, each with a placeholder test. Configure signing with a free Apple Development certificate (owner creates it in Xcode; document the steps in `docs/signing.md`). Move the Tauri app to `legacy/` without changing it. SwiftLint and `swift-format` config. CI: `xcodebuild build test` on a macOS runner plus the existing legacy checks.
**Acceptance:** app launches to an empty window; `xcodebuild test` runs one test per package in CI; Tauri app still builds from `legacy/`.

### SW-02 · Design system package · L (UI part gated on DS-05)
Tokens from Paper (colours, radii, shadows, spacing), Fraunces, Geist and Geist Mono bundled and registered with `CTFontManager`, components: Pill, Button, Card, Toggle, NavItem, Segmented control, Chip, Level meter, HUD surface.
**Acceptance:** a gallery view shows every component in all states; contrast checked against the Paper values; no hard-coded colours outside the package.

### SW-03 · Database and migrations · L
GRDB in `MinutaeCore`. Opens `~/Library/Minutae/minutae.db` in WAL mode. Migration v1 recognises a Tauri-created database (Drizzle tables) and does not touch its data; migration v2 adds `dictations`, `jobs`, `tags`, `item_tags`, `dictionary`, `transcript_edits` and the `api_usage_log` columns. Records and repositories per table. `ValueObservation` helpers.
**Acceptance:** tests open a copy of a real Tauri database fixture and read meetings, segments and summaries; v2 migration is idempotent; WAL and busy timeout set; no data loss test.

### SW-04 · Settings and Keychain · M
Typed settings store (replaces the key-value `settings` table usage), Keychain wrapper for API keys with a fake, `codexModel`, hotkey, retention, paths.
**Acceptance:** keys never appear in the database or logs; round-trip tests; defaults match ARCHITECTURE.

### SW-05 · Job runner · L
`JobRunner` actor over the `jobs` table: bounded concurrency per kind, retry with backoff, restart recovery (`running` → `queued`), cancellation, observable state.
**Acceptance:** tests for crash recovery, retry, concurrency limits and cancel, using fake job handlers.

### SW-06 · Permission service · M
Status and request for Microphone, Accessibility, Screen Recording/audio capture, Calendars. Observable. Plain-language reasons. No UI beyond a debug view until designed.
**Acceptance:** fake-backed tests; real status read works in the app; each denial returns a defined fallback value.

### SW-07 · App shell, panels, status item · L (UI gated)
`NSStatusItem` with template icon and recording variant, popover `NSPanel`, HUD `NSPanel` base class, main `Window` that hides on close, app keeps running, Launch at login.
**Acceptance:** HUD base never takes focus (test with a script and the frontmost app check from SPIKE-01); status item icon switches state from a fake recorder; no Dock-quit surprises.

---

## M1 — AI without API keys

### LLM-02 · Transport protocol and subprocess runner · L
Deps: SW-01. `LLMTransport`, `LLMRequest`, `LLMResponse`, `LLMError`, `SubprocessRunner` per ARCHITECTURE §5.2–5.3: real binary path resolution, stdin prompt, empty `0700` temp cwd, timeout, cancel, single-flight, stderr classification, output redaction in logs.
**Acceptance:** fake-CLI tests (a small script standing in for each vendor) cover success, bad JSON, non-zero exit, timeout, cancel, rate-limit text, not-installed, not-logged-in; no shell is ever invoked.

### LLM-04 · Codex CLI transport · M
Deps: LLM-02, SW-04. Use the flags from `docs/llm-providers.md`, including `--ignore-user-config`, `--ignore-rules`, `--ephemeral`, `-o`, and `--model` from `codexModel`. Parse JSONL events. A model rejection becomes `modelRejected`.
**Acceptance:** fake-CLI tests; with an empty `codexModel` the Test action returns a clear classified error; one real probe passes once the owner supplies the model.

### LLM-05 · Grok CLI transport · M
Deps: LLM-02. Flags from the doc; JSON parse; classified errors.
**Acceptance:** fake-CLI tests; real `pong` probe recorded in the PR.

### LLM-06 · Ollama transport · M
Deps: LLM-02. `POST /api/chat` to localhost, non-streaming first; detect installed and running; detect whether any installed model is truly local; "no local model" is a classified state with a one-click pull of a small model that the user must confirm.
**Acceptance:** tests against a local fake server; status distinguishes server down, no model, cloud-only model, ready.

### LLM-07 · API transport (Advanced) · M
Deps: LLM-02, SW-04. Anthropic, OpenAI and Gemini over `URLSession`, keys from Keychain, only used when the user enables it. Cost computed from usage.
**Acceptance:** fake-server tests; key never logged; off by default.

### LLM-08 · Router, fallback and JSON repair · L
Deps: LLM-04/05/06. Fallback chain, per-purpose policy, JSON validation with one repair retry, map-reduce chunking for long transcripts.
**Acceptance:** tests for every fallback rule in ARCHITECTURE §5.2, including "never fall back on cancel", and chunking on a 2-hour fixture.

### LLM-09 · Usage accounting · S
Deps: SW-03, LLM-08. Write `transport`, `duration_ms`, `error_class` per call; queries for the Usage page.
**Acceptance:** aggregates test; no prompt text stored.

### LLM-10 · Provider settings UI · M (gated on DS-03)
Provider cards with status and Test, fallback order, privacy labels, Advanced API disclosure.
**Acceptance:** matches the approved design; Test shows the classified error text.

---

## M2 — Dictation

### DIC-01 · Hotkey service · M
`HotkeyService` using Carbon `RegisterEventHotKey` for ⌥Space press and release; hold and toggle modes; user-changeable chord; failure reporting and fallback suggestion.
**Acceptance:** works with no Input Monitoring; verified that Option+Space does not type a non-breaking space in TextEdit, Notes, Safari, VS Code and Slack; verified on macOS 14.4 (or the closest available OS, recorded in the PR with an explicit gap note).

### DIC-02 · Audio capture · L
Deps: SW-06. `AVAudioEngine` input, ring buffer with pre-roll, VAD trim, 16 kHz mono conversion, incremental WAV write, shared with meetings through one engine and two consumers.
**Acceptance:** tests on fixture audio for conversion and VAD; crash mid-capture leaves a playable WAV up to the last 10 s.

### DIC-03 · Speech engine · L
Deps: SW-01. `SpeechEngine` protocol; `WhisperCppEngine` links whisper.cpp as a library (Metal), warm model in an actor, idle unload, model manager that validates names and paths (no path injection), language setting.
**Acceptance:** 10 s fixture transcribes in ≤ 1.5 s p50 on Apple Silicon with the tiny or base model; test with a fake engine for everything above it.

### DIC-04 · Post-processing and dictionary · M
Deps: SW-03. Local clean-up (spacing, casing, punctuation), dictionary words and snippets, optional LLM clean-up through the router (off by default).
**Acceptance:** golden-file tests; dictionary applies per app scope.

### DIC-05 · Text inserter · L
Deps: SW-06. Pasteboard plus ⌘V with full save and restore (all types), AX insertion, `CGEvent` typing, `IsSecureEventInputEnabled()` refusal, frontmost app capture, per-app method memory, clipboard fallback with a clear result.
**Acceptance:** the insertion matrix from SPIKE-01 filled in for TextEdit, Notes, Safari, VS Code, Slack, with method per app recorded in `docs/insertion-matrix.md`; secure-field refusal demonstrated in a password field; pasteboard restored byte-for-byte in a test.

### DIC-06 · Dictation controller and persistence · M
Deps: DIC-01…05, SW-03. State machine (idle, listening, processing, inserting, error), Esc to cancel, one at a time, save row and WAV under `~/Library/Minutae/dictations/YYYY-MM/`, link to the target app.
**Acceptance:** state machine tests with fakes; an error at any stage still saves what exists; every dictation appears in the database.

### DIC-07 · Dictation HUD · M (gated on DS-04)
**Acceptance:** matches the design states; never steals focus; appears within 150 ms of key press.

### DIC-08 · Dictation settings and Dictate page · M (gated on DS-03)
**Acceptance:** hotkey picker, insert method, keep-audio toggle, dictionary link, permission rows, all matching the design.

---

## M3 — Meetings (parity with the Tauri app, then better)

### MTG-01 · Recording pipeline · L
Deps: DIC-02, SW-05. Start, pause, stop with atomic stop (finalise WAV, write row, enqueue jobs in one transaction).
**Acceptance:** crash test; stop never loses audio; durable jobs created.

### MTG-02 · Transcription job · M
Deps: MTG-01, DIC-03. Whisper job with progress, segments and word timing saved to `segments`.
**Acceptance:** fixture meeting produces segments; restart mid-job resumes.

### MTG-03 · Summary job and markdown export · L
Deps: LLM-08, MTG-02. Structured summary with decisions and action items, written to `~/Library/Minutae/summaries/` with the same frontmatter format as today.
**Acceptance:** output files match the old format byte-for-byte for a fixture; classified errors surface in the UI model.

### MTG-04 · Detection and prompt · M
Core Audio mic-running monitor, quiet-hours and per-app rules, prompt panel (gated on approved design, already approved).
**Acceptance:** debounce tests; prompt never steals focus.

### MTG-05 · Them audio (process taps) · L
Deps: MTG-01, SW-06. Tap the process that actually plays audio (not the browser main PID), mix to a second track, mic-only fallback when denied.
**Acceptance:** works with Music, Spotify, Zoom and Chrome (helper process) on the current OS and on 14.4 or an explicit gap note; denial never crashes.

### MTG-06 · Calendar awareness · M
EventKit titles and attendees, linked to a meeting when overlapping.
**Acceptance:** denial path works; no calendar data leaves the Mac.

### MTG-07 · Live notes · L (gated, approved)
Streaming partial transcript during recording.
**Acceptance:** latency target in ARCHITECTURE §10; does not slow the final transcription.

---

## M4 — Library

### LIB-01 · Search · L
FTS5 and embeddings (NLContextualEmbedding first, relevance test decides vs bundled Core ML), cosine with Accelerate, ⌘K search.
**Acceptance:** relevance test on a labelled fixture; results sanitised.

### LIB-02 · Library UI · L (gated on DS-04)
### LIB-03 · Transcript editor · L (gated on DS-04)
Edits go to `transcript_edits`, originals never overwritten; speaker rename.
### LIB-04 · Export · M
Markdown, TXT, SRT, JSON. Tests for file name sanitisation.
### LIB-05 · Retention and storage · M
Audio-only purge, default never, storage usage page.
### LIB-06 · Re-run and versioning · M
Re-run transcription or summary with another model, keep versions.
### LIB-07 · Ask across meetings · M
Retrieval, then the router; cites sources.
**Acceptance for LIB-02…07:** match the approved designs; each has tests for its data logic.

---

## M5 — UI port

### UI-01…UI-06 · Screens · L each (all gated on approved design)
Meetings and detail, Summary and Transcript, Dictate, Settings, Usage, Onboarding with permission flow, tray popover, prompt card, recording pill, Search modal.
**Acceptance:** each matches the Paper artboard; empty, loading and error states present; accessibility labels and keyboard navigation checked.

---

## M6 — Quality and release

### QA-01 · Test suites · L
Unit tests per package, fake-backed integration tests, a UI smoke test per screen, CI on every PR.
### QA-02 · Performance pass · M
Verify ARCHITECTURE §10 targets on Apple Silicon.
### REL-01 · Signing, DMG, updater · L
No paid account. Sign with the free Apple Development certificate, hardened runtime on, build a DMG, document the right-click Open first launch in the README, optional Sparkle with an EdDSA appcast, entitlements review. Notarisation is out of scope until an account exists.
### MIG-01 · Data compatibility · M
A real-data migration test with copies of existing user databases.
### MIG-02 · Retire Tauri · S
Tag `tauri-final`, delete `legacy/`, update `CLAUDE.md` and `README.md`.
### REL-02 · Docs · S

---

## Suggested order

1. HK-01, SW-01, SW-03, SW-04 (parallel after SW-01).
2. SW-05, SW-06, LLM-02, DIC-03.
3. Lanes in parallel: LLM-04/05/06 → LLM-08; DIC-01, DIC-02, DIC-05 → DIC-06; MTG-01 → MTG-02 → MTG-03.
4. UI tasks as each DS row flips to approved.
5. M6.

## Risk register

| Risk | Mitigation |
|---|---|
| Permissions reset on every build | free Apple Development certificate from day one (SW-01), never ad-hoc |
| Option-only hotkey breaks on some macOS versions | test 14.4 and current, fallback chord, user-changeable |
| Text insertion differs per app | per-app method memory, clipboard fallback, insertion matrix |
| Process taps need the right PID and permission | audio-process discovery, mic-only fallback |
| Whisper latency regresses with larger models | warm actor, tiny/base default, measure in QA-02 |
| Vendor CLI flags or terms change | transports isolated, Test action, status check on launch, Ollama fallback |
| Rewrite scope | parity milestone M3 before polish, Tauri kept until MIG-02 |
