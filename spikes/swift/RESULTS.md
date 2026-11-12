# SPIKE-01 results

Machine: Apple M5 · macOS 27.0.1 (Build 26A434) · Xcode 27.2 · Swift 6.4.2  
Date: 2026-11-13  
Branch: `spike/swift-native`  
Scope: throwaway code under `spikes/swift/` only. Tauri app untouched.

## Recommendation

**Go Swift, with these exceptions.** The pieces that are painful or impossible in a WKWebView — global ⌥Space without Input Monitoring, a non-activating HUD, Core Audio process taps, and spawning vendor CLIs — all work from a small Swift app on this machine. Whisper.cpp via `Process` already beats the 1.5s warm-dictation budget on M5 with `ggml-tiny`. Stay on Tauri only if the rewrite cost of the existing React library UI outweighs that native gap; the risky APIs themselves are not a reason to stay. Exceptions: (1) Accessibility TCC is required for reliable text insertion and was not grantable from this agent session; (2) WhisperKit was not latency-measured because spike rules forbid a Hub download and no local CoreML model was present; (3) ad-hoc signing does not keep TCC grants stable across rebuilds — use a Development/Developer ID identity before shipping; (4) process taps need Screen Recording and a real audio-producing process object (Chrome’s browser PID alone returned `!obj`).

---

## 1. Hold-to-talk hotkey — **works** (NBSP interactive confirm blocked by TCC)

| | |
|---|---|
| API | Carbon `RegisterEventHotKey(kVK_Space, optionKey)` |
| Input Monitoring | **Not required.** Registration succeeded with status `0`. |
| Permissions prompted | None for the hotkey itself. |
| NBSP into focused app | **Not observed in this session.** `osascript` / System Events keystrokes were denied (`not allowed to send keystrokes`). Carbon hotkeys are expected to consume ⌥Space so a NBSP is not typed; owner should confirm once in TextEdit with `MinutaeSpike.app` running. |
| Evidence | Auto-demo log: `hotkey_register_status: 0`. |

## 2. Dictation overlay — **works**

| | |
|---|---|
| API | `NSPanel` with `.nonactivatingPanel`, `.canJoinAllSpaces`, `.fullScreenAuxiliary`, `ignoresMouseEvents = true`, `orderFrontRegardless()`, never `NSApp.activate` |
| Focus steal | **No.** Auto-demo: `overlay: show listening; app_active=false key=false` then processing, then hide; final `isActive=false`. |
| States | `listening` and `processing` labels shown. |
| Manual “type in another app while visible” | Not driven end-to-end here (Computer Use / Accessibility denied). Code path and `app_active=false` are the evidence. |

## 3. Text insertion — **partly works**

| Method | Result |
|---|---|
| `IsSecureEventInputEnabled()` gate | **Works.** Checked; was `false` outside a secure field. Insertion path returns early when `true`. Could not force a live password-field `true` without Accessibility to focus the field. |
| Clipboard + Cmd+V with pasteboard save/restore | **Partly works.** Code saves/restores `NSPasteboard`. `CGEvent` Cmd+V posted, but TextEdit content stayed `BEFORE\|` — posting HID events from an untrusted CLI does not land. Expect the same until Accessibility (or a proper accessibility client) is granted to the **app** binary. |
| Accessibility `AXSelectedText` / `AXValue` | **Fails here:** `ax_trusted: false`. Error surfaced clearly. |
| Apps matrix (TextEdit, Notes, Safari, Slack/VS Code) | **Not completed** — blocked on TCC. Code is ready; grant Accessibility to `com.nebula.minutae.spike` and re-run `spike-cli insertion ax\|clipboard` with each app focused. |

## 4. Speech latency — **whisper.cpp works / WhisperKit fails (no local model)**

Audio: 10s `say` → 16 kHz mono WAV (not live mic; `SPIKE_USE_MIC=1` available). Transcripts omitted from the repo.

### (b) whisper.cpp via Swift `Process` → `whisper-cli`

| | Run 1 | Run 2 |
|---|---|---|
| Chip | Apple M5 | Apple M5 |
| Model | `ggml-tiny.bin` (local, Application Support) | same |
| Time to first text | 0.662 s (batch; equals total) | 0.668 s |
| Total time | **0.662 s** | **0.668 s** |
| RSS delta | ~0.2 MB (child process; parent RSS) | ~0.2 MB |
| vs 1.5 s target | **Met** for this short clip with tiny, cold-ish process spawn | Met |

### (a) WhisperKit

| | |
|---|---|
| Result | **Fails** under spike network rules |
| Why | No `SPIKE_WHISPERKIT_MODEL_FOLDER`; `SPIKE_ALLOW_MODEL_DOWNLOAD` not set. Hub fetch would be required for `tiny` CoreML. |
| Code | Compiles against WhisperKit 0.18.0; path ready when a local model folder exists. |

## 5. Per-app audio (process tap) — **works** (with caveats)

| | |
|---|---|
| API | `AudioHardwareCreateProcessTap` + `CATapDescription` (macOS 14.2+) |
| Current OS (27.0.1) | **Works** for Music (`create_status: 0`, `tap_id` allocated) and Spotify (`create_status: 0`). |
| Chrome browser PID 142 | **Fails** — `process_object_id: 0`, `create_status: 560947818` (`!obj`). Need the helper process that actually plays audio, not the main browser PID. |
| macOS 14.4 | **Not separately tested** (this machine is 27.0.1). API is available since 14.2; treat 14.4 as supported on paper, verify on a 14.4 device before locking the min OS. |
| Permissions | Screen Recording is typically required. Microphone alone is not enough for other apps’ audio. |
| If user denies | No crash; `AudioHardwareCreateProcessTap` returns a non-zero `OSStatus`. No “Them” stream until permission is granted and the app is relaunched. |

## 6. LLM bridge — **works**

Flags match `docs/llm-providers.md`. Prompt via stdin/file, empty temp cwd, timeout, cancel, classified errors. No vendor config dirs read. No skip-permissions flags.

| Case | Result |
|---|---|
| `grok` | **Works.** Exit 0, JSON `text: "pong"`, model `grok-4.6-build`. Classified `unknown` on success (success is not a failure code). |
| `codex` without `MINUTAE_CODEX_MODEL` | **Works as error path.** `invalid_output: set MINUTAE_CODEX_MODEL (or CODEX_MODEL) — model is not hardcoded`. |
| `codex` with model | **Not run.** Owner supplies the model name separately; not guessed. |
| Cancel | **Works.** `Process.terminate()` → exit 15 → `cancelled`. |
| Timeout | Implemented (`--timeout`); not separately exercised beyond cancel. |

## 7. Menu bar — **works**, with design gap

| | |
|---|---|
| API | SwiftUI `MenuBarExtra` + SF Symbol (`waveform.circle` / `record.circle.fill`) |
| Idle / recording | Label/icon swap on `isRecording`. |
| Popover vs Paper | **Limitation:** `.menuBarExtraStyle(.menu)` is a system menu, not the Paper popover HUD. `.window` is closer but still not an arbitrary designed NSPanel. For a true Paper popover, use `NSStatusItem` + custom `NSPanel`, or accept system menu chrome. |
| Template icon | SF Symbols behave as template; dual custom PDF templates not bundled in this spike. |

## 8. Packaging — **works** (ad-hoc)

| | |
|---|---|
| Artifact | `spikes/swift/AppBundle/MinutaeSpike.app` via `Scripts/package-app.sh` |
| Sign | `codesign --sign -` → `Signature=adhoc`, `CDHash=f058d57414a036759b59d744df2d98f98ebcd6b2` (rebuilds change this). |
| Bundle id | `com.nebula.minutae.spike` |
| TCC across rebuilds | Ad-hoc CDHash changes → Mic / Accessibility / Screen Recording often re-prompt. Stable Development or Developer ID identity needed for “permissions stick”. |
| Notarization | Needs Apple Developer Program + `notarytool` + staple. |
| Sparkle | Same as any Mac app: EdDSA key, appcast, notarized updates. No Swift-specific blocker. |

---

## Open questions for the owner (not invented here)

1. Default `MINUTAE_CODEX_MODEL` / `codexModel` setting value for Codex ChatGPT logins.
2. Confirm ⌥Space does not type NBSP in TextEdit with the spike app running (TCC blocked automation here).
3. Grant Accessibility to the spike app and fill the insertion matrix (TextEdit / Notes / Safari / Slack or VS Code × clipboard vs AX).
4. Whether WhisperKit may download CoreML models on first run in product builds, or must ship/offline-cache them.
5. Min OS still 14.4 — re-verify process taps on a real 14.4 machine.
