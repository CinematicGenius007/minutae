// IPC contract: types shared by the Tauri backend (Rust, serde camelCase) and the UI.
// Source of truth for phase 2. Rust DTOs in apps/desktop/src-tauri/src/dto.rs must serialize to exactly these shapes.
// Times are ISO-8601 strings; durations/offsets are milliseconds.

export type Uuid = string;
export type Os = "macos" | "windows" | "linux";

export type MeetingStatus = "recording" | "processing" | "done" | "failed";
export type Track = "mic" | "system";
export type Stage = "transcribing" | "diarizing" | "summarizing" | "indexing" | "downloadingModel";

export type FailureKind =
  | "notInstalled" | "notLoggedIn" | "rateLimited" | "timeout" | "cancelled" | "invalidOutput"
  | "modelRejected" | "unavailable" | "noProviderReady" | "permissionDenied" | "modelMissing" | "io" | "internal";

export interface Failure { kind: FailureKind; message: string; fix: string | null }

export interface Progress { meetingId: Uuid | null; stage: Stage; fraction: number; etaSeconds: number | null }

export interface Meeting {
  id: Uuid; title: string; startedAt: string; endedAt: string | null;
  status: MeetingStatus; sourceApp: string | null; failure: Failure | null;
}

export interface MeetingListItem {
  id: Uuid; title: string; startedAt: string; durationMs: number | null; status: MeetingStatus;
  /** One-line summary, or a status line while processing. */
  takeaway: string | null;
  people: string[];
  openActions: number; totalActions: number;
  progress: Progress | null; failure: Failure | null;
}

/** "Today", "Yesterday", "Earlier this week", "Last week", "November 2026"... computed by backend. */
export interface DayGroup { label: string; items: MeetingListItem[] }

export interface WeekStats { recordedMs: number; openActions: number }

export interface Speaker { id: Uuid; meetingId: Uuid; label: string; displayName: string | null; track: Track }
export interface Segment { startMs: number; endMs: number; text: string; track: Track; speakerId: Uuid | null }
export interface ActionItem { id: Uuid; text: string; owner: string | null; due: string | null; done: boolean }
export interface Moment { meetingId: Uuid; atMs: number; label: string }

export interface Summary {
  overview: string; points: string[]; decisions: string[]; takeaway: string;
  markdown: string; markdownPath: string | null; provider: string; model: string;
}

export interface MeetingDetail {
  meeting: Meeting;
  durationMs: number | null;
  speakers: Speaker[];
  segments: Segment[];
  summary: Summary | null;
  actionItems: ActionItem[];
  moments: Moment[];
  /** Absolute file paths; play through `audioUrl()` (convertFileSrc). */
  audio: { mic: string | null; system: string | null } | null;
  progress: Progress | null;
}

export type RecordingPhase = "idle" | "recording" | "paused" | "stopping";
/** Live captions for this recording: on, turned off in Settings, or no Whisper model downloaded. */
export type CaptionStatus = "on" | "off" | "noModel";
export interface RecordingState {
  phase: RecordingPhase; meetingId: Uuid | null; title: string | null; sourceApp: string | null;
  elapsedMs: number; systemAudio: "on" | "off" | "unavailable"; systemAudioError: string | null;
  captions: CaptionStatus;
}
export interface LevelFrame { elapsedMs: number; mic: number; system: number }
/**
 * `recording://caption`: one live caption line (best effort; the full transcript comes after stop).
 * A later line with the same `id` replaces it (partial → final). A final line with empty `text`
 * removes it. `speaker` is "You" (mic) or "Them" (system audio). `atMs`/`endMs` are on the
 * recording timeline (pauses excluded).
 */
export interface CaptionLine {
  id: number; meetingId: Uuid; track: Track; speaker: string;
  atMs: number; endMs: number; text: string; final: boolean;
}

export interface Dictation {
  id: Uuid; createdAt: string; text: string; rawText: string; targetApp: string | null;
  wordCount: number; durationMs: number; inserted: boolean;
}
export interface DictationStats { wordsThisWeek: number; minutesSaved: number }

export type HudState =
  | { state: "idle" }
  | { state: "listening"; level: number; handsFree: boolean }
  | { state: "processing" }
  | { state: "inserted"; words: number }
  | { state: "copied"; words: number; reason: string }
  | { state: "error"; message: string };

export type SearchKind = "all" | "meetings" | "dictations" | "actionItems";
export interface SearchHit {
  kind: "meeting" | "dictation" | "actionItem" | "summary";
  meetingId: Uuid | null; dictationId: Uuid | null;
  title: string; snippet: string; highlights: [number, number][]; atMs: number | null; date: string;
}
export interface Answer { text: string; sources: SearchHit[]; provider: string }

export type Appearance = "system" | "light" | "dark";
export type WhisperModel = "tiny" | "base" | "small" | "medium";
export type Transport = "codexCli" | "grokCli" | "ollama" | "api";

export interface AppSettings {
  appearance: Appearance; launchAtLogin: boolean; autoDetectMeetings: boolean; promptDelaySeconds: number;
  stopWhenCallEnds: boolean; includeSystemAudio: boolean; microphone: string | null; recordShortcut: string;
  transcribeWhere: "local" | "cloud"; whisperModel: WhisperModel; language: string | null; labelSpeakers: boolean;
  /** Live captions while recording (only when a Whisper model is downloaded). */
  liveCaptions: boolean;
  transportOrder: Transport[]; apiFallbackEnabled: boolean; summaryLength: "brief" | "detailed";
  summariesPath: string | null; codexModel: string | null; ollamaModel: string;
  dictationHotkey: string; dictationMode: "hold" | "toggle"; tidyDictation: boolean; keepDictationAudio: boolean;
  vocabulary: string[]; deleteAudioAfterDays: number | null; recordingsPath: string | null;
}

export interface InputDevice { id: string; name: string; isDefault: boolean }

export type PermissionState = "granted" | "denied" | "notDetermined" | "notRequired" | "unsupported";
export interface Permissions { microphone: PermissionState; systemAudio: PermissionState; accessibility: PermissionState }

export interface PlatformInfo { os: Os; sessionKind: "x11" | "wayland" | null; appVersion: string }

export type ModelState = "missing" | "downloading" | "ready";
export interface ModelInfo { model: WhisperModel; sizeBytes: number; state: ModelState; fraction: number | null }
export interface DiarizeModelInfo { state: ModelState; fraction: number | null; sizeBytes: number }

export type Availability = "ready" | "notInstalled" | "notLoggedIn" | "unavailable";
export interface ProviderStatus { transport: Transport; name: string; availability: Availability; detail: string | null; model: string | null }
export type ApiProvider = "anthropic" | "openai" | "gemini" | "grok";

export interface UsageByProvider { provider: string; calls: number; tokens: number; costUsd: number }
export interface UsageMonth {
  year: number; month: number; totalUsd: number; summaries: number; tokens: number;
  /** 0..1 share of calls that ran on-device (free). */
  freeShare: number; byProvider: UsageByProvider[]; byDay: { day: number; costUsd: number }[];
}

export interface StorageUsage { recordingsBytes: number; modelsBytes: number; indexBytes: number; dataDir: string }

export type Route =
  | "/" | "/live" | "/dictate" | "/library" | "/onboarding" | "/gallery"
  | `/meetings/${string}` | `/settings/${SettingsSection}`;
export type SettingsSection = "general" | "transcription" | "summaries" | "dictation" | "storage" | "usage";
