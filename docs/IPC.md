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
3. Windows load the same bundle: `main` → `#/`, `hud` → `#/hud`, `prompt` → `#/prompt`.
4. Commands return quickly. Long work (transcribe, diarize, summarize, model download) runs as a job and reports through events (`job://progress`, `model://progress`, `meetings://changed`).

## Global shortcuts (record, dictation)

| | |
|---|---|
| `shortcut_status()` → `ShortcutStatus` | `backend`: `native` (tauri-plugin-global-shortcut; keys come from `recordShortcut` / `dictationHotkey`), `portal` (Wayland: XDG `GlobalShortcuts` portal; the desktop owns the keys and `dictateTrigger` / `recordTrigger` are its descriptions), `unavailable` (Wayland without the portal). `holdSupported: false` once the portal is seen not to send key releases: dictation then runs press-to-toggle. `detail` says why the portal failed |
| `configure_shortcuts()` | Opens the desktop's own shortcut settings (portal `ConfigureShortcuts`, interface v2+; `canConfigure`). Errors with one sentence otherwise |
| `shortcuts://changed` | `ShortcutStatus`, after the portal binds or fails, and when release support becomes known |

The portal is chosen at startup when the session is Wayland (`MINUTAE_SHORTCUTS=portal|native` overrides). With the portal, `update_settings` still stores `dictationHotkey` / `recordShortcut` (sent as the preferred trigger next launch) but does not register them.

Command line, forwarded to the running app by tauri-plugin-single-instance (or handled at launch): `minutae --toggle-dictation`, `minutae --toggle-recording`. These are the workaround when `backend` is `unavailable`: bind them to custom shortcuts in the desktop's keyboard settings.

Mock switches for these screens: `?os=linux&session=wayland` (portal), `&portal=press` (no releases), `&portal=none` (unavailable).
