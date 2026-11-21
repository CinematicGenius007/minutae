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
