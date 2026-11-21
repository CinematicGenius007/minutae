// IPC contract: every command and event between the UI and the Rust backend.
// Command names are the Rust `#[tauri::command]` function names (snake_case); args are camelCase
// (Tauri converts). Outside Tauri (plain `vite dev`, screenshot tests) calls go to `mock.ts`.

import type {
  Answer, AppSettings, ApiProvider, CaptionLine, DayGroup, DiarizeModelInfo, Dictation, DictationStats,
  HudState, InputDevice, LevelFrame, MeetingDetail, Moment, ModelInfo, Permissions, PlatformInfo, Progress,
  ProviderStatus, RecordingState, SearchHit, SearchKind, StorageUsage, UsageMonth, Uuid, WhisperModel, Route,
  Meeting, UpdateStatus,
} from "./types";
import { mockInvoke, mockListen } from "./mock";

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri) return mockInvoke<T>(cmd, args);
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export type Unlisten = () => void;
async function on<T>(event: string, cb: (payload: T) => void): Promise<Unlisten> {
  if (!inTauri) return mockListen<T>(event, cb);
  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(event, (e) => cb(e.payload));
}

export const api = {
  // app
  platform: () => call<PlatformInfo>("platform"),
  getSettings: () => call<AppSettings>("get_settings"),
  updateSettings: (patch: Partial<AppSettings>) => call<AppSettings>("update_settings", { patch }),
  permissions: () => call<Permissions>("permissions"),
  requestPermission: (kind: keyof Permissions) => call<Permissions>("request_permission", { kind }),
  openSystemSettings: (kind: keyof Permissions) => call<void>("open_system_settings", { kind }),
  revealPath: (path: string) => call<void>("reveal_path", { path }),
  openPath: (path: string) => call<void>("open_path", { path }),
  copyText: (text: string) => call<void>("copy_text", { text }),
  /** Path → URL playable by <audio>. */
  audioUrl: async (path: string) => {
    // The browser mock hands out data: URLs as "paths"; real files only play inside Tauri.
    if (!inTauri) return path.startsWith("data:") ? path : "";
    const { convertFileSrc } = await import("@tauri-apps/api/core");
    return convertFileSrc(path);
  },
  onboardingDone: () => call<boolean>("onboarding_done"),
  finishOnboarding: () => call<void>("finish_onboarding"),
  /** Show the main window (at `route` if given) and close the menu-bar panel. */
  openMain: (route?: Route) => call<void>("open_main", { route: route ?? null }),
  quitApp: () => call<void>("quit_app"),

  // macOS menu-bar panel (window "tray")
  trayPanelHide: () => call<void>("tray_panel_hide"),
  /** The panel reports its content height in logical px; the window is resized to fit. */
  trayPanelResize: (height: number) => call<void>("tray_panel_resize", { height }),

  // meetings
  listMeetings: (limit = 200) => call<DayGroup[]>("list_meetings", { limit }),
  weekStats: () => call<{ recordedMs: number; openActions: number }>("week_stats"),
  getMeeting: (id: Uuid) => call<MeetingDetail>("get_meeting", { id }),
  renameMeeting: (id: Uuid, title: string) => call<void>("rename_meeting", { id, title }),
  deleteMeeting: (id: Uuid) => call<void>("delete_meeting", { id }),
  retryMeeting: (id: Uuid) => call<void>("retry_meeting", { id }),
  setActionDone: (id: Uuid, done: boolean) => call<void>("set_action_done", { id, done }),
  renameSpeaker: (speakerId: Uuid, name: string) => call<void>("rename_speaker", { speakerId, name }),
  summaryMarkdown: (id: Uuid) => call<string>("summary_markdown", { id }),

  // recording
  recordingState: () => call<RecordingState>("recording_state"),
  startRecording: (opts: { title?: string; sourceApp?: string } = {}) => call<Meeting>("start_recording", opts),
  stopRecording: () => call<Uuid>("stop_recording"),
  pauseRecording: () => call<void>("pause_recording"),
  resumeRecording: () => call<void>("resume_recording"),
  markMoment: (label?: string) => call<Moment>("mark_moment", { label: label ?? null }),
  acceptCallPrompt: () => call<Meeting>("accept_call_prompt"),
  dismissCallPrompt: (always?: boolean) => call<void>("dismiss_call_prompt", { always: always ?? false }),
  listInputDevices: () => call<InputDevice[]>("list_input_devices"),

  // dictation
  listDictations: (limit = 100) => call<Dictation[]>("list_dictations", { limit }),
  dictationStats: () => call<DictationStats>("dictation_stats"),
  deleteDictation: (id: Uuid) => call<void>("delete_dictation", { id }),

  // library
  search: (query: string, kind: SearchKind = "all") => call<SearchHit[]>("search", { query, kind }),
  ask: (question: string) => call<Answer>("ask", { question }),

  // models
  listModels: () => call<ModelInfo[]>("list_models"),
  downloadModel: (model: WhisperModel) => call<void>("download_model", { model }),
  cancelModelDownload: (model: WhisperModel) => call<void>("cancel_model_download", { model }),
  deleteModel: (model: WhisperModel) => call<void>("delete_model", { model }),
  diarizeModel: () => call<DiarizeModelInfo>("diarize_model"),
  downloadDiarizeModel: () => call<void>("download_diarize_model"),

  // providers
  providerStatuses: () => call<ProviderStatus[]>("provider_statuses"),
  setApiKey: (provider: ApiProvider, key: string) => call<void>("set_api_key", { provider, key }),
  clearApiKey: (provider: ApiProvider) => call<void>("clear_api_key", { provider }),
  /** Masked hint like "AIza••••3kQ", or null when no key. */
  apiKeyHint: (provider: ApiProvider) => call<string | null>("api_key_hint", { provider }),

  // usage + storage
  usageMonth: (year: number, month: number) => call<UsageMonth>("usage_month", { year, month }),
  storageUsage: () => call<StorageUsage>("storage_usage"),
  pickFolder: () => call<string | null>("pick_folder"),
  exportAll: () => call<string | null>("export_all"),
  eraseAll: () => call<void>("erase_all"),

  // updates
  updateStatus: () => call<UpdateStatus>("update_status"),
  /** Starts a check; returns "checking" at once. The result arrives on `events.updateStatus`. */
  checkForUpdates: () => call<UpdateStatus>("check_for_updates"),
  /** Downloads + installs the found update, then the app restarts. Rejects while recording. */
  installUpdate: () => call<UpdateStatus>("install_update"),
};

/** Backend → UI events. Event names are the strings below. */
export const events = {
  recordingState: (cb: (s: RecordingState) => void) => on("recording://state", cb),
  /** ~30 Hz while recording. */
  levels: (cb: (f: LevelFrame) => void) => on("recording://levels", cb),
  caption: (cb: (c: CaptionLine) => void) => on("recording://caption", cb),
  progress: (cb: (p: Progress) => void) => on("job://progress", cb),
  /** Any meeting row changed (status, title, summary...). Refetch list/detail. */
  meetingsChanged: (cb: (p: { meetingId: Uuid | null }) => void) => on("meetings://changed", cb),
  dictationsChanged: (cb: () => void) => on("dictations://changed", cb),
  callDetected: (cb: (p: { app: string | null }) => void) => on("call://detected", cb),
  callEnded: (cb: () => void) => on("call://ended", cb),
  hud: (cb: (s: HudState) => void) => on("dictation://hud", cb),
  modelProgress: (cb: (p: { model: WhisperModel | "diarize"; fraction: number; done: boolean; error: string | null }) => void) =>
    on("model://progress", cb),
  settingsChanged: (cb: (s: AppSettings) => void) => on("settings://changed", cb),
  /** Tray / menu / shortcut asks the main window to navigate. */
  navigate: (cb: (p: { route: Route }) => void) => on("nav://goto", cb),
  /** Updater phase / progress changed. */
  updateStatus: (cb: (s: UpdateStatus) => void) => on("update://status", cb),
  /** macOS menu-bar panel was just opened: refresh its data. */
  trayShown: (cb: () => void) => on("tray://shown", cb),
};
