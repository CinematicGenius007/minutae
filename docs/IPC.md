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
