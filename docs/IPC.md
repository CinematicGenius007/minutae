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
