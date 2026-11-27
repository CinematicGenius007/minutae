# IPC contract (UI ↔ Rust)

The contract is code, not prose:

| File | Holds |
|---|---|
| `apps/desktop/src/lib/types.ts` | Every payload shape (camelCase JSON) |
| `apps/desktop/src/lib/api.ts` | Every command (`api.*` → Rust `#[tauri::command]` snake_case name) and every event name (`events.*`) |
| `apps/desktop/src/lib/mock.ts` | In-browser fake backend with Paper content, used outside Tauri (`vite dev`, screenshot tests). `?state=empty` and `?state=recording` URL switches |

Rules:

1. Rust DTOs (`apps/desktop/src-tauri/src/dto.rs`) serialize to exactly the TS shapes. If the backend cannot provide a field, it sends `null`; it never renames.
2. A contract change edits `types.ts`, `api.ts` and `mock.ts` in the same change, plus the Rust side.
3. Windows load the same bundle: `main` → `#/`, `hud` → `#/hud`, `prompt` → `#/prompt`, and on macOS only `tray` → `#/tray` (menu-bar panel; `open_main`, `tray_panel_resize`, `tray_panel_hide`, `quit_app`, event `tray://shown`).
4. Commands return quickly. Long work (transcribe, diarize, summarize, model download) runs as a job and reports through events (`job://progress`, `model://progress`, `meetings://changed`).
5. Import from older Minutae apps (MIG-10): `import_legacy_scan` → `LegacyScan` (sources found at their default paths, counts, audio a run would copy, `running`). `import_legacy_run` returns at once; the run reports `import://progress` (`ImportProgress`, throttled to ~10/s) and one `import://finished` (`ImportFinished`: `ImportResult` or an error), then `meetings://changed` and `dictations://changed`. It is not a `minutae-jobs` job: re-running is safe because `legacy_imports` maps every legacy row to its v4 id. Mock switches: `?import=none|error|running|failed`.
6. Updates (`apps/desktop/src-tauri/src/updater.rs`): `update_status`, `check_for_updates` and `install_update` return an `UpdateStatus` at once; the result, download progress and errors arrive on `update://status`. The webview never calls `tauri-plugin-updater` directly (its JS commands are not in the capability). `UpdateStatus` also carries `channel` (`direct` / `scoop` / `source`), `canInstall` and `updateCommand`: only a `direct` build with an updater public key installs updates itself; every other copy checks the GitHub API and the UI shows `updateCommand` (`scoop update minutae`, …) with a Copy button. Phase `disabled` means checks are off (`MINUTAE_UPDATE_CHECK=off`); the UI shows the version and no button. `install_update` refuses while recording and restarts the app through the normal graceful quit.

## Live captions (`recording://caption`)

Best effort, on-device, while a recording is running. The full transcript is still made from the
complete files after stop; captions are never saved.

- `RecordingState.captions` says what the Live screen can expect: `"on"`, `"off"` (turned off with
  `AppSettings.liveCaptions`) or `"noModel"` (no Whisper model downloaded). Flipping the setting
  during a recording applies at once (if a model exists).
- Each event is a `CaptionLine`: `id`, `meetingId`, `track` (`mic` | `system`), `speaker`
  (`"You"` | `"Them"`), `atMs`/`endMs` on the recording timeline (pauses excluded), `text`, `final`.
- **Same `id` = same line.** A partial (`final: false`) is replaced by later events with its id; the
  final one settles it. A final with empty `text` removes the line (it turned out to be silence or
  a repeat).
- Lines from both tracks interleave; order them by `atMs`.
- Pausing stops captions; partials on screen are settled as final. When the machine falls behind,
  older audio is skipped, so gaps in captions are normal.

Backend: `minutae_asr::captions` (engine, trait-driven, tests with a fake transcriber) fed by
`minutae_audio::Recorder::start_with_taps`; wiring in `apps/desktop/src-tauri/src/recording.rs`.

## Global shortcuts (record, dictation)

| | |
|---|---|
| `shortcut_status()` → `ShortcutStatus` | `backend`: `native` (tauri-plugin-global-shortcut; keys come from `recordShortcut` / `dictationHotkey`), `portal` (Wayland: XDG `GlobalShortcuts` portal; the desktop owns the keys and `dictateTrigger` / `recordTrigger` are its descriptions), `unavailable` (Wayland without the portal). `holdSupported: false` once the portal is seen not to send key releases: dictation then runs press-to-toggle. `detail` says why the portal failed |
| `configure_shortcuts()` | Opens the desktop's own shortcut settings (portal `ConfigureShortcuts`, interface v2+; `canConfigure`). Errors with one sentence otherwise |
| `shortcuts://changed` | `ShortcutStatus`, after the portal binds or fails, and when release support becomes known |

The portal is chosen at startup when the session is Wayland (`MINUTAE_SHORTCUTS=portal|native` overrides). With the portal, `update_settings` still stores `dictationHotkey` / `recordShortcut` (sent as the preferred trigger next launch) but does not register them.

Command line, forwarded to the running app by tauri-plugin-single-instance (or handled at launch): `minutae --toggle-dictation`, `minutae --toggle-recording`. These are the workaround when `backend` is `unavailable`: bind them to custom shortcuts in the desktop's keyboard settings.

Mock switches for these screens: `?os=linux&session=wayland` (portal), `&portal=press` (no releases), `&portal=none` (unavailable).

## Echo cancellation

- Setting `AppSettings.echoCancellation`: `"auto"` (default) or `"off"`. Auto cleans the mic track with the system track as the far-end reference before transcription, and skips when there is no system track, it is silent, or the speakers can't be heard in the mic (headphones). Changing it applies to the next transcription (including Retry).
- `MeetingDetail.echoCancel: EchoCancelReport | null`, `null` for meetings transcribed before the step existed:
  `{ ran, skipped, erleDb, delayMs, driftPpm, elapsedMs }`. `ran: true` means the transcript came from `recordings/<id>/mic.clean.wav` (the original `mic.wav` is kept and is what the player plays). `skipped` is one of `"off" | "noSystemTrack" | "systemSilent" | "noEcho" | "ineffective"`; `erleDb` is how much quieter the speaker echo got.
- Rust: `minutae_core::{EchoCancelMode, EchoCancelReport, EchoSkip}`, `dto::MeetingDetail::echo_cancel`, stored in `meetings.echo_cancel` (schema v4).

## Dictation shapes and HUD

Settings persist `dictationDefaultShape`, `dictationShapeEdits`, `dictationAppRules`, and
`dictationStartSound` (off by default). With no explicit default preset, `tidyDictation: true` uses Tidy
and `false` uses the legacy cleanup-off rules, bypassing saved Tidy edits. Cleanup-off preserves
capitalization and still handles spoken punctuation and vocabulary. Updating the toggle alone clears an explicit default.
Resolution order is user app rule, automatic Verbatim for recognized terminals, then the
default. A quick switch applies to the current dictation only. History keeps the start app's
`appIdentity` and the actual `shape`, including edited rules. Older rows return null for both.

| Command | Contract |
|---|---|
| `dictation_shape_options` | `DictationShapeOptions`: factory presets, ordered quick presets, effective default and capabilities. Local rewrite is unavailable. Cloud rewrite is `planned`. |
| `list_dictation_app_rules` | Saved `DictationAppRule[]` with display names. |
| `save_dictation_app_rule(rule)` / `remove_dictation_app_rule(matcher)` | Upsert or remove by app matcher. Returns `AppSettings` and emits `settings://changed`. |
| `list_dictation_apps` | `DictationApp[]` with matcher, display name and nullable suggested preset. Combines history with running macOS apps, visible Windows apps or X11 window classes when EWMH and `xprop` are available. Wayland supplies no foreground app metadata. |
| `set_dictation_shape_edits(preset, edits)` | Save per-preset capitalization and single-line final-period settings. Pass null for Reset shape. Verbatim and Code & commit ignore both edits to protect identifiers, including edits already saved or supplied to Preview. |
| `preview_dictation_shape(sample, preset, edits)` | Runs the Rust rules and current vocabulary. Omit edits or pass null to use saved edits. Pass an object for a draft (empty object for factory rules). |
| `get_dictation_hud` / `dictation://hud` | Snapshot and event stream, both `HudState`. Listening has the app label and preset. `shortcutPresets` lists successful temporary bindings for the current modifier combination and can be empty. Use `quickPresets` for the digit order (Verbatim, Chat message, Email, Notes and lists, Code & commit). |
| `switch_dictation_shape(preset)` | Switch to a quick preset during listening. Option/Alt+1–5 is registered only during listening on native shortcut backends. While the dictation hotkey is down, its modifiers are part of the combination. |
| `set_dictation_hud_interaction(hovered, focused)` | Set pointer-hover and keyboard-focus-within state together. Either pauses the remaining six-second timer for Inserted and Copied. Error has no timer. |
| `dismiss_dictation_hud` | Hides a result or error. While listening or processing, requests cancellation. |
| `undo_dictation_insert(resultId)` | Requires a visible Inserted result with `undoAvailable: true`, a matching result ID and the original process instance still frontmost. Native paste does not confirm insertion, so the native HUD currently reports Undo unavailable. |
| `fix_dictation_error` | Performs the current Error state's `fixAction`. Retry starts a hands-free session. |

The HUD is visible on macOS, Windows, X11 and Wayland. Use `capabilities.hudInteractive` to
gate pointer controls. macOS enables first-mouse clicks on its non-activating NSPanel, and
Windows handles `WM_MOUSEACTIVATE` on its non-activating HUD. X11 and Wayland use a passive,
click-through HUD with GTK focus hints. Wayland placement and visibility depend on the compositor.
Errors stay visible until dismissed or replaced by a new dictation. An interactive error pill
offers a Dismiss button. A passive HUD exposes Dismiss dictation error in the native tray menu
only while an error is visible.

An insert stages text on the clipboard and sends Command+V on macOS or Ctrl+V on Windows and
X11. Minutae checks the foreground app again immediately before sending the keystroke and
copies when it detects a change. Focus can still change between the check and OS delivery.
Identified terminals get clipboard-only results for text
containing a line break or other control character. Paste delivery errors also leave the
result on the clipboard.

- **macOS** copies when Accessibility is untrusted, secure keyboard entry is active,
  the focused Accessibility role or subrole is `AXSecureTextField`, or the role is a
  known non-text control. Password fields that expose neither secure keyboard entry nor the
  secure role or subrole are undetected, including fields with missing, unreadable or
  misreported Accessibility attributes. Unknown and custom roles can receive a paste.
- **Windows** copies when a focused Win32 `Edit` has `ES_PASSWORD` or reports a
  nonzero mask through `EM_GETPASSWORDCHAR`. Password fields in non-`Edit` controls
  are undetected, including browser and Electron fields, RichEdit, Office and WinUI.
  A password field is also undetected when the focused control or its password metadata
  cannot be read and neither check gives a positive signal.
- **Linux X11** and **unknown sessions** attempt the keystroke. Linux does not inspect
  the focused field for passwords. X11 uses the EWMH active window ID to detect focus changes,
  including between windows with the same class. When IDs are unavailable, it compares available
  app identities. The class also identifies terminals. The app picker excludes windows with
  Minutae’s process ID when EWMH supplies it. **Wayland** always copies without inspecting the field or injecting input.

Keystroke delivery cannot confirm that the app accepted the paste or that its next Undo
would undo Minutae's edit. The native HUD reports Undo unavailable on all platforms.

When enabled, the start sound plays while the microphone is already recording so
capture starts immediately. Playback through speakers can reach the mic and affect
transcription. Leave the sound off to keep the cue out of the dictated audio.

Browser fixtures use `?state=listening|processing|inserted|inserted-no-undo|copied|error#/hud`
(or `?hud=...`). Other fixtures include `listening-handsfree`, `listening-unknown`,
`copied-secure`, `error-transcription`, `error-retry` and `error-insertion`. Browser
previews simulate the shape editor's design sample. The native Rust command processes
arbitrary samples.
