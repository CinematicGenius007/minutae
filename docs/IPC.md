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

## Meeting tape and content management

- `meeting_waveform_levels(id, buckets, range?) → MeetingWaveform` returns `mic` (“You”) and
  `system` (“Others”) on the same millisecond range. Each ready track has exactly 1–2048
  requested peak levels from 0–1. A shorter track is zero-padded. Omit `range` for the whole tape
  or pass a segment’s `{ startMs, endMs }` for the line. `missing` means no track was registered,
  `deleted` means a registered file is gone or its deletion was recorded, and `unreadable` includes
  a reason. A recorded deletion skips disk access and clears the cached levels.
- Playback uses `getMeeting(id).audio`, `audioUrl(path)` and the existing player’s
  `seek(segment.startMs, true)`. Follow playback with `currentMs` and the segment’s half-open
  interval. A timestamp can still be displayed when audio is unavailable.
- `add_action_item(id, item)`, `edit_action_item(id, itemId, item)` and
  `delete_action_item(id, itemId)` manage the checklist. Add and edit return a
  `SavedActionItem { item, markdownNotice }`; delete and the existing `set_action_done` return
  `ActionChangeResult { markdownNotice }`. `ActionItemInput` is a full replacement for text,
  owner, due and the optional `sourceMs`. Omitted or null optional fields clear their value.
  Edit preserves completion. Every change is saved in the app. In the Markdown file only
  checklist lines change; an export that is byte for byte what Minutae saved, but has no
  Action Items section, gains one. `markdownNotice` is a sentence to show the user when an
  existing file was left unchanged: it was edited in a way that can't be merged safely, no
  single checklist can be located, it can't be verified as this meeting's export, or it was
  kept after the in-app summary was deleted. Otherwise it is null. A saved summary whose own
  checklist is ambiguous (older imports) keeps its text and never blocks a change. A command
  fails, with the database and file unchanged, only on a real read or write failure such as an
  unavailable folder. An edit leaves an absent export absent.
- **Hand edits to the checklist.** Each add, edit, delete or tick compares three versions: the
  checklist Minutae last wrote, the file now and the app's items. Nothing happens to a file
  until one of those commands (or `set_action_done`) runs; the result may then hold more items
  than the command touched, so refetch the meeting on `meetings://changed`.
  - A task line (`- [ ]`, `* [x]`, `+ [ ]`) that Minutae did not write becomes an action item
    with its tick state, after the item above it. Its line stays as typed.
  - A hand-changed tick is taken into the app. Reworded text is taken too when the line sits
    between the same unchanged neighbours and shares at least half its words with the old
    text. Owner, due date and ` · m:ss` timestamp are kept while the new wording shows them.
    A tick changed on one side and wording on the other combine.
  - An item deleted in the app has its line removed, even if its tick was changed by hand. A
    line Minutae removed or replaced for the app never comes back as a new item.
  - Notes, prose, plain bullets, comments and subsections stay where they are.
  - The file is left byte for byte unchanged, with `markdownNotice`, while the app change is
    still saved and nothing is taken from the file, when: a line was removed by hand and its
    item still exists; a rewrite can't be matched to one line; the app and the file changed
    the same wording or tick differently; a line holds wording the app removed earlier (an
    older copy of the file, or the same task typed again); a task is nested, quoted, numbered,
    or continues on another line; or the section holds other text but no task. Later changes
    merge again once the file no longer conflicts, for example after deleting in the app an
    item whose line was removed by hand.
  - The record of the last written checklist is `meetings.exported_checklist` (schema v7). For
    exports made before it, the saved summary Markdown stands in. Deleting the summary clears it.
- `summarize_again(id)` queues a summary job for a finished meeting with a saved transcript.
  The meeting stays `done` with progress while the job runs. On failure, `meeting.failure` reports
  the error and the previous summary and export remain available. A successful rerun writes the
  new export. When the existing file differs from the last saved Markdown, Minutae first saves
  those bytes to a sibling `*.edited-<uuid>.md` copy that it will never replace or clean up. A
  task typed into the file but not yet taken into the app is in that copy, not in the new
  export. If the saved path is missing and belongs to a previous folder, the rerun writes to
  the current summaries folder. Until the first user checklist change, generated items can be replaced. After any
  add, edit, delete or completion change, the entire checklist is curated. Reruns preserve its
  rows, IDs, timestamps, completion and deletions. On upgrade, a checklist containing any
  completed item is already considered curated.
- `meeting_contents(id) → MeetingContents` lists `audio`, `transcript`, `summary` and
  `markdown`. File sizes are measured on disk. SQLite parts show UTF-8 content payload sizes,
  independent of database pages and indexes. A null size indicates a file could not be measured.
  `blockedReason` explains why a part cannot be removed. `markdownNotice` is a message when no
  export was found in known folders, otherwise null. On upgrade Minutae searches the current
  and default folders in the background and records verified paths. It remembers previous
  configured folders from then on. A newly remembered folder is searched once for meetings with
  no saved path; busy meetings are returned to later, and opening, changing or deleting a
  meeting searches its known folders first. Changing storage settings and starting a recording
  wait for none of this. If a known folder is offline, legacy exports with no saved path remain
  pending until the folder is available. Custom folders changed before this upgrade may be
  unknown to Minutae.
- `delete_meeting_parts(id, parts) → MeetingDeletion` accepts any nonempty selection on a saved
  meeting. Audio, transcript, in-app summary and Markdown are independent. The summary includes
  decisions, action items and marks. An export kept during summary deletion remains readable.
  When all known content is gone the meeting row is removed. The result gives successful parts,
  failures and the refreshed ledger. If Markdown was selected and no export was found,
  `markdownNotice` explains the absence and Markdown is omitted from `deleted`. File cleanup uses the default or configured storage folders
  and locations saved for the meeting, including folders used before a settings change.
  User-chosen storage roots may resolve through symlinks. Cleanup opens files and directories
  below the root without following symlinks. Exports are moved into place without replacing
  anything, including on FAT, exFAT and network disks. It removes only recorder-owned names from the
  meeting's ID directory, plus the exact imported audio filename when an import record verifies
  it. Non-empty directories are left in place. Markdown must have a complete frontmatter block
  with exactly one matching meeting ID. If cleanup is interrupted, the next file operation can
  recover an adjacent `.minutae-pending-…` folder. After successful deletion the saved path is cleared. If a storage folder needed to locate a part is
  unavailable, the part and its meeting record remain available for retry. Deletion waits for
  active processing and retires queued work before changing inputs. Retiring jobs does not make
  audio eligible for retention. A meeting whose processing was retired moves to `failed` and can
  be retried explicitly.
- `recording_discard_info(id) → RecordingDiscardInfo` reads active duration, WAV bytes on disk,
  track count, visible caption line count and marks. It can be refreshed while the prompt is
  open. `discard_recording(id) → RecordingDiscard` confirms for that ID, stops capture without
  stitching or queuing processing, and removes the meeting from the library. Cleanup uses the
  folder selected when recording began. If cleanup fails, including when that folder is
  unavailable, `cleanupError` explains why and Minutae keeps a pending cleanup record to retry
  on startup.

- `erase_all()` applies the same file checks across known storage folders and pending discards,
  then erases the library and resets settings. It retains file cleanup records for unavailable
  meeting storage and retries them on startup. If anything could not be removed, the command
  reports the affected locations or errors even if the library was erased.

Browser mock switches include `?tape=ready|missing|deleted|unreadable`,
`?meetingDelete=blocked|partial`, `?resummarize=failed|running`, `?checklist=unchanged` and
`?state=recording&discard=blocked|cleanupError`.

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
| `set_dictation_hud_interaction(hovered, focused)` | Set pointer-hover and keyboard-focus-within state together. Either pauses the remaining six-second timer for Inserted and Copied. Error has no timer. Idle, Listening and Processing reset it. No current control calls it, and Inserted and Copied receive no pointer events. |
| `dismiss_dictation_hud` | Hides a result or error. While listening or processing, requests cancellation. No current control calls it. |
| `dismiss_dictation_error` | Hides the HUD only while its state is Error; otherwise does nothing, so a late click never cancels a newer dictation. Used by the error pill's Dismiss button; the tray item does the same. |
| `undo_dictation_insert(resultId)` | Requires a visible Inserted result with `undoAvailable: true`, a matching result ID and the original process instance still frontmost. Native paste does not confirm insertion, so the native HUD currently reports Undo unavailable. |
| `fix_dictation_error` | Performs the current Error state's `fixAction`. Retry starts a hands-free session. |

The HUD is visible on macOS, Windows, X11 and Wayland, and never takes focus from the app being
dictated into. It is created click-through on every platform.

| Platform | Clickable HUD (`capabilities.hudInteractive`) |
|---|---|
| macOS | Yes: a non-activating `NSPanel`. `false` if the panel style cannot be applied. |
| Windows | No. A click on the WebView2 child window could activate the HUD despite `WS_EX_NOACTIVATE`. |
| Linux X11 | No. GTK focus hints and an empty input region. |
| Linux Wayland | No. Same as X11; placement and visibility depend on the compositor. |

A clickable HUD accepts the pointer only while the current state renders a control the user can
press (today: Error). In every other state, including Listening, Processing, Inserted and Copied,
clicks pass through to the app underneath. While it accepts the pointer, the whole 320×64 HUD
window does, not only the pill. `hud_accepts_pointer` in `shell.rs` holds this rule; a state that
gains a control must be added there. Gate pointer controls on `capabilities.hudInteractive`.

Errors stay visible until dismissed or replaced by a new dictation. A clickable error pill
offers a Dismiss button. A passive HUD (Windows and Linux) exposes Dismiss dictation error in the
native tray menu only while an error is visible.

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
