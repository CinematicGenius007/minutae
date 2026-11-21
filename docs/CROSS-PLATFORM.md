# Minutae v4: cross-platform plan (Tauri 2 + Rust)

Written 2026-11-20. Decided by the owner on 2026-11-20:

- One app for **macOS, Windows and Linux**. SwiftUI is dropped.
- Stack: **Tauri 2**. A Rust core does audio, AI and storage; a React + TypeScript UI renders the "On air" design. Bun is the frontend toolchain.
- Built as a **new app**. The Swift packages (`Packages/`) and the old Tauri app (`legacy/`) stay as reference until the new app reaches parity. Nothing is deleted before then.

Read with:

- [`DESIGN-LANGUAGE.md`](./DESIGN-LANGUAGE.md): rules and tokens
- The Paper file: https://app.paper.design/file/01M7W5R5J6SBFEN8K99EKT2NX0 (pictures)
- [`REDESIGN.md`](./REDESIGN.md) §1, §4, §5: screen list and behaviour

## 0. Why the last Tauri app looked bad, and what changes

The UI quality of a Tauri app is the quality of its HTML/CSS. Tauri itself does not limit it. The old app had no design language, no component kit, no visual checks against the design, and it captured audio inside the webview (MediaRecorder).

This time:

1. **The design language is written first.** Every value comes from tokens (`DESIGN-LANGUAGE.md`).
2. **One component kit** (`libs/ui`) implements every row of the Paper "Components" sheet before any screen is built.
3. **Screens are checked against Paper** with screenshot tests in light and dark.
4. **All audio, AI and storage live in Rust.** The webview only draws, and it never blocks on heavy work.

---

## 1. Repository layout

New code goes in new top-level folders. `Packages/`, `App/`, `Minutae.xcodeproj` and `legacy/` stay untouched until parity.

```
apps/
  desktop/                 Tauri 2 app
    src/                   React 19 + TS (routes, screens)
    src-tauri/             Rust: commands, windows, tray, plugins (thin)
packages/
  ui/                      tokens.css, fonts, React components, gallery
crates/
  minutae-core/            types, settings, paths, errors
  minutae-store/           SQLite (migrations, FTS5, repositories)
  minutae-jobs/            persistent job runner (tokio)
  minutae-audio/           capture: mic + system audio per OS, chunked writer, level meter
  minutae-detect/          "another app is using the mic" per OS
  minutae-asr/             whisper.cpp (whisper-rs), model manager
  minutae-diarize/         speaker diarization (sherpa-onnx), speaker profiles
  minutae-llm/             router: codex-cli, grok-cli, ollama, API (port of MinutaeLLM)
  minutae-dictation/       hotkey state machine, post-processing, text insertion
  minutae-search/          FTS + local embeddings, retrieval for "Ask"
```

Toolchain:

- Bun workspaces for JS
- Cargo workspace for Rust
- Biome for TS
- rustfmt and clippy for Rust

---

## 2. Frontend

| Concern | Choice | Why |
|---|---|---|
| Framework | React 19 + TypeScript strict, Vite | Known, fast HMR |
| Styling | Plain CSS with tokens (`tokens.css` custom properties) + CSS modules | Exact match to the design language. No utility-class drift |
| Accessible primitives | Radix UI (Select, DropdownMenu, Tabs, Switch, Checkbox, RadioGroup, Dialog, Toast) | Keyboard and screen-reader support out of the box. Styled entirely by us |
| Icons | Lucide | Matches the stroke rules in §6 of the design language |
| Routing | TanStack Router | Typed routes |
| State | Zustand for UI; backend state pushed from Rust via Tauri events | Rust is the source of truth |
| Fonts | Geist, Geist Mono, Newsreader bundled as woff2 | Identical rendering on every OS |

The UI gets data only through typed commands and events (generated with `tauri-specta`), never by polling. The live recording screen subscribes to a level/timer event stream at 30 Hz at most.

---

## 3. Native capabilities per OS

This is the part that decides whether the design is "honestly implementable".

| Capability | macOS | Windows | Linux | Crate / API |
|---|---|---|---|---|
| Microphone | CoreAudio | WASAPI | PipeWire / Pulse / ALSA | `cpal` |
| Other people's audio (system output) | Core Audio process tap (14.4+), ScreenCaptureKit fallback | WASAPI loopback | PipeWire monitor source | `cpal` (Win, Linux); `objc2` bindings (macOS) |
| Detect a call (mic used by another app) | Core Audio process objects (`kAudioProcessPropertyIsRunningInput`, 14+), else `kAudioDevicePropertyDeviceIsRunningSomewhere` | Capability Access Manager consent store (mic in-use entries) | PipeWire / Pulse source-outputs | own code |
| Transcription | whisper.cpp + Metal | whisper.cpp + Vulkan/CUDA, CPU fallback | same as Windows | `whisper-rs` |
| Who said what (diarization) | sherpa-onnx (pyannote segmentation + speaker embedding), CPU / CoreML | sherpa-onnx CPU / CUDA | sherpa-onnx CPU | official `sherpa-onnx` crate |
| Global hold-to-talk hotkey | Yes (needs Accessibility) | Yes | X11 yes; Wayland through the GlobalShortcuts portal (desktop-dependent, see below) | `tauri-plugin-global-shortcut` (pressed/released); Wayland: `ashpd` (zbus, pure Rust) |
| Type text into other apps | Paste via clipboard + simulated ⌘V, then restore the clipboard | Same with Ctrl+V | X11 yes; Wayland: copy plus a toast fallback | `enigo`, `arboard` |
| HUD / prompt that never steals focus | NSPanel | Non-activating, always-on-top window | Best effort | `tauri-nspanel` (macOS), window flags |
| Tray / menu bar | Menu bar extra | Tray | AppIndicator | Tauri tray |
| Secrets (API keys) | Keychain | Credential Manager | Secret Service | `keyring` |
| Open at login, updater, notifications | Yes | Yes | Yes | Tauri plugins |

**Wayland shortcuts.** X11 key grabs don't work on Wayland, so on a Wayland session (`XDG_SESSION_TYPE` / `WAYLAND_DISPLAY`; `MINUTAE_SHORTCUTS=portal|native` overrides) the app binds "dictate" and "record" through `org.freedesktop.portal.GlobalShortcuts` (`minutae-dictation::shortcuts`, `platform/portal.rs`) instead of the plugin:

- The user's accelerators go to `BindShortcuts` as preferred triggers only; the desktop shows its own dialog and owns the keys. Settings shows the trigger it returned and, when the portal is version 2+, a "Change in system settings" button (`ConfigureShortcuts`).
- `Activated` → press, `Deactivated` → release, so hold-to-talk works where the compositor sends both. If a second `Activated` arrives with no `Deactivated` in between, or `Deactivated` arrives under 20 ms after `Activated`, releases are treated as missing: dictation switches to press-to-toggle for the session and Settings says so.
- No portal (older GNOME, some compositors): Settings explains it and offers `minutae --toggle-dictation` / `--toggle-recording` to bind as custom desktop shortcuts. tauri-plugin-single-instance forwards the flags to the running app.

**macOS system audio, in detail (`minutae-audio`):**

- During a call, the mic monitor (`minutae-detect`) reports the process using the mic (pid + bundle id). The recording then captures **only that app's audio**, so music and videos stay out. `target::resolve` collects the app's audio processes: the process itself, processes with the same bundle root (`com.google.Chrome.helper*` → `com.google.Chrome`), and children or processes the app is "responsible" for (Safari's `com.apple.WebKit.GPU`). Shared WebKit ids and terminal siblings of CLI tools are excluded.
- **14.4+:** `CATapDescription initStereoMixdownOfProcesses` → `AudioHardwareCreateProcessTap` → a private aggregate device (default output as clock + the tap) → an IOProc that downmixes into the rtrb ring (no allocation). `kAudioHardwarePropertyTranslatePIDToProcessObject` is the fallback lookup.
- **13.0–14.3:** ScreenCaptureKit (`SCStream`, `capturesAudio`, `excludesCurrentProcessAudio`), with a content filter that includes only the call app's `SCRunningApplication`s, or the whole display if the app is not found. Needs Screen Recording; onboarding asks for it on those versions (`CGPreflight/RequestScreenCaptureAccess`). `MINUTAE_MAC_SYSTEM_AUDIO=sck|tap` forces a backend for testing.
- **Fallback:** no call app, no audio process yet, or tap failure → all system audio (cpal global tap). Once a second the capture thread re-resolves: it switches to the app once it has audio, adds new helper processes, and rebuilds when the output device changes. `RecordingHandle::system_source()` and `CaptureEvent::SystemSourceChanged` report the mode. The UI shows "Mic + Zoom audio" vs "Mic + system audio" (`RecordingState.systemAudioScope`).
- CoreAudio is weak-linked (`-weak_framework CoreAudio` in the build scripts) so the tap symbols (14.2+) don't stop the app launching on 13.x.

**Prior art:** Meetily (open source, Tauri + Rust) records mic + system audio and transcribes locally on all three OSes using the same APIs (WASAPI loopback, ScreenCaptureKit/CoreAudio, Pulse monitor sources). So the approach is proven, not hypothetical.

### Multi-speaker plan

1. **Record two tracks**, not one:
   - **mic** = "You"
   - **system** = everyone else

   This alone answers "who said it" for 1:1 calls, with no AI.
2. **Diarize the system track** with sherpa-onnx into Speaker 1…N.
3. **Merge with timestamps:**
   - Transcribe both tracks with Whisper.
   - Assign each segment its speaker from diarization overlap.
   - Interleave the two tracks by time.
4. **Names:**
   - The user renames "Speaker 2" to "Alice" once, in the transcript.
   - Optionally, a local voice profile (an embedding, never audio) suggests "Alice?" in later meetings.
   - Profiles are opt-in and deletable.
5. **In-room meetings** (one laptop mic, many people): diarize the mic track the same way.

---

## 4. Data and paths

| OS | App data |
|---|---|
| macOS | `~/Library/Minutae/` (unchanged, so existing recordings stay put) |
| Windows | `%APPDATA%\Minutae\` |
| Linux | `$XDG_DATA_HOME/minutae/` |

- **Database:** SQLite (`rusqlite`), WAL mode, versioned migrations.
  - Migration v1 opens the existing Tauri/Drizzle and Swift (GRDB) databases read-only and imports them.
  - Tables: meetings, tracks, segments, speakers, speaker_profiles, summaries, action_items, dictations, jobs, api_usage_log, settings.
- **Audio:** 10-second chunks per track, so a crash loses at most 10 s. After the meeting the chunks are joined into FLAC.
- **Markdown summaries:** same format as today (YAML frontmatter), written to a user-chosen folder.

---

## 5. Performance targets

| Metric | Target |
|---|---|
| App size (no models) | < 25 MB |
| Cold start to usable window | < 1 s |
| Idle CPU with detection on | < 1% |
| CPU while recording (without live captions) | < 5% |
| Data loss on crash while recording | ≤ 10 s |
| 30 min meeting → transcript + speakers + summary | < 3 min on Apple Silicon, < 6 min on a mid-range Windows laptop (CPU) |
| Dictation: release key → text appears (10 s clip, base model) | ≤ 1.5 s |
| Hotkey → HUD visible | ≤ 150 ms |
| ⌘K / Ctrl+K keyword results | < 100 ms |
| Meetings list with 1,000 rows | 60 fps scroll (virtualised) |

---

## 6. Risks and how each is handled

| Risk | Handling |
|---|---|
| Linux WebKitGTK renders CSS differently or slower | Bundled fonts; no exotic CSS (no `backdrop-filter`); screenshot test run on Linux in CI |
| Wayland limits global hotkeys and typing into apps | Detect Wayland; use the GlobalShortcuts portal (toggle when the compositor sends no releases; CLI flags when there is no portal); fall back to "copied, press Ctrl+V" with a toast. X11 works fully |
| macOS < 14.4 has no process taps | ScreenCaptureKit audio fallback (needs the Screen Recording permission). Document 13+ as the minimum |
| Diarization quality varies | Two-track recording removes most of the problem; spike measures accuracy on real recordings first |
| GPU acceleration differs per machine | whisper.cpp CPU path always works; GPU is detected and optional; model size suggested by hardware |
| Unsigned builds scare users | Phase 1 is developer builds; signing per OS in the release milestone |

---

## 7. Milestones and tasks

Same rules as before: one task ID per PR, acceptance criteria, tests with fakes, CI green on all three OSes.

### M0 — Foundation (about 1 week)

| ID | Task | Size | Acceptance |
|---|---|---|---|
| X-01 | Workspace: Bun + Cargo workspaces, `apps/desktop` Tauri 2 shell, CI matrix (macOS, Windows, Ubuntu) building and testing | M | Empty window builds and launches on all 3 OSes in CI |
| X-02 | `libs/ui` tokens + fonts + theme switch (system/light/dark) | S | Token values equal `DESIGN-LANGUAGE.md` §13; theme switches live |
| X-03 | Component kit: every row of the Components sheet, built on Radix | L | Gallery route shows all components × all states × both themes; keyboard works |
| X-04 | Screenshot tests (Playwright, WebKit + Chromium) for the gallery | M | Baselines committed; CI fails on a diff |
| X-05 | App shell: sidebar, 4 routes, record pill, ⌘/Ctrl shortcuts, tray, window chrome per OS | M | Matches 01 artboard empty state in both themes |

### SPIKE-02 — Prove the hard parts (2–3 days, runs alongside M0)

- Record mic + system audio on macOS, Windows and Ubuntu (PipeWire).
- Transcribe with whisper-rs.
- Diarize with sherpa-onnx on 3 real calls.

**Acceptance:** a written results file with accuracy, speed and CPU per OS. The owner decides go/adjust before M1.

### M1 — Capture

| ID | Task | Size |
|---|---|---|
| AUD-01 | Mic capture, device list, level meter stream | M |
| AUD-02 | System audio per OS (tap / loopback / monitor) | L |
| AUD-03 | Chunked crash-safe writer, two tracks, finalise to FLAC | M |
| DET-01 | Call detection per OS + prompt panel (non-activating) | L |
| UI-02 | Live recording screen | M |

### M2 — Understand

| ID | Task | Size |
|---|---|---|
| ASR-01 | whisper-rs, model download/manager (Settings → Transcription) | L |
| DIA-01 | Diarization + two-track merge + speaker table | L |
| DIA-02 | Rename speakers; opt-in voice profiles | M |
| JOB-01 | Persistent job runner with retry and progress events | M |

### M3 — Summarise

| ID | Task | Size |
|---|---|---|
| LLM-10 | Port the router (codex-cli, grok-cli, ollama, API) with the same failure kinds | L |
| SUM-01 | Summary, decisions, action items, markdown export, usage log | M |
| UI-01 | Meetings list | M |
| UI-03 | Meeting detail | L |

### M4 — Dictate

| ID | Task | Size |
|---|---|---|
| DIC-10 | Hold-to-talk hotkey state machine, HUD | M |
| DIC-11 | Post-processing + vocabulary | M |
| DIC-12 | Text insertion per OS with fallbacks | L |
| UI-04 | Dictate page | M |

### M5 — Library and settings

| ID | Task | Size |
|---|---|---|
| LIB-10 | FTS + local embeddings | L |
| LIB-11 | Ask with sources | M |
| UI-05 | Library page | M |
| UI-06 | Settings: 6 pages | L |

### M6 — Ship

| ID | Task | Size |
|---|---|---|
| ONB-01 | First run + permissions per OS | M |
| MIG-10 | Import existing databases and recordings | M |
| REL-10 | Signing: macOS (free cert first), Windows (Authenticode later), Linux AppImage + .deb; updater | L |
| QA-10 | Performance pass against §5; accessibility pass | M |

Rough timeline for one agent: about 8–10 weeks. UI tasks run in parallel with backend tasks once X-03 lands.

---

## 8. Ground rules for implementers

1. No UI before X-03 (component kit) is merged. Screens only compose kit components.
2. No raw colours, font sizes or spacing in screen code. Tokens only.
3. The webview never does audio, AI or file work. It calls a command and listens for events.
4. Every Rust service has a trait and a fake; tests never need a mic, model, CLI or permission.
5. Every screen ships with empty, loading, error and both-theme screenshots.
6. Platform gaps are handled with a designed fallback and copy, never a silent failure.
