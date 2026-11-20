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
5. Updates (`apps/desktop/src-tauri/src/updater.rs`): `update_status`, `check_for_updates` and `install_update` return an `UpdateStatus` at once; the result, download progress and errors arrive on `update://status`. The webview never calls `tauri-plugin-updater` directly (its JS commands are not in the capability). `UpdateStatus` also carries `channel` (`direct` / `scoop` / `source`), `canInstall` and `updateCommand`: only a `direct` build with an updater public key installs updates itself; every other copy checks the GitHub API and the UI shows `updateCommand` (`scoop update minutae`, …) with a Copy button. Phase `disabled` means checks are off (`MINUTAE_UPDATE_CHECK=off`); the UI shows the version and no button. `install_update` refuses while recording and restarts the app through the normal graceful quit.
