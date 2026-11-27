// In-browser fake backend. Used when the UI runs outside Tauri (vite dev, screenshot tests).
// Content mirrors the Paper artboards so screens can be compared 1:1.
// URL switches (query string before the hash, e.g. `/?state=failed#/meetings/<id>`):
//   ?state=empty       empty lists (Meetings, Dictate, Library)
//   ?state=recording   start in a live recording
//   ?state=processing  the Detail screen shows a meeting still transcribing (no summary yet)
//   ?state=failed      the Detail screen shows a meeting whose summary failed (reason + fix)
//   ?onboarding=0      first run: onboarding_done() is false, so the shell sends you to /onboarding
//   ?os=macos          report this OS instead of guessing from the user agent (macos | windows | linux)
//   ?session=wayland   Linux session kind (x11 | wayland); wayland uses the GlobalShortcuts portal
//   ?portal=none       with session=wayland: no portal, shortcuts unavailable (CLI workaround shown)
//   ?portal=press      with session=wayland: the portal sends no key releases, so hold works as toggle
//   ?perms=none        every permission is notRequired (Windows / Linux): onboarding has nothing to ask
//   ?audio=1           meetings have short silent audio tracks, so the player is enabled
//   ?captions=off      live captions turned off (also: noModel = no Whisper model; none = on, nothing said yet)
//   ?import=none       no older Minutae data (also: error, running, failed; see legacyScan below)
//   ?dictations=loading|error|errorOnce   force Dictate list loading or error state (errorOnce recovers on retry)
//   ?update=available|checking|error|disabled   start the updater in that phase (default idle)
//   ?channel=scoop|source   a copy that can't install updates itself and shows its update command
//
// Behaviour mirrors the real backend where the UI must cope with it:
//   - request_permission("systemAudio" | "accessibility") on macOS can keep answering notDetermined
//   - update_settings refuses bare-key and duplicate global shortcuts with an error string
//   - stop_recording reports phase "stopping" for ~600 ms before the meeting is saved
//   - ask() answers take a different time per question, so a later question can land first

import type {
  Answer, AppMatcher, AppSettings, DictationApp, DictationAppRule, DictationCapabilities, DictationShapeOptions, HudState, Shape, ShapeEdits, ShapePreset, CaptionLine, CaptionStatus, DayGroup, Dictation, ImportResult, LegacyScan, LegacySource, MeetingDetail,
  ModelInfo, Permissions, ProviderStatus, RecordingState, SearchHit, ShortcutStatus, Track,
  UpdateStatus, UsageMonth,
} from "./types";

type Listener = (payload: unknown) => void;
const listeners = new Map<string, Set<Listener>>();
const emit = (event: string, payload: unknown) => listeners.get(event)?.forEach((l) => l(payload));

const params = typeof window !== "undefined" ? new URLSearchParams(window.location.search) : new URLSearchParams();
const state = params.get("state");
const empty = state === "empty";
const osParam = params.get("os");
const os = osParam === "macos" || osParam === "windows" || osParam === "linux"
  ? osParam
  : typeof navigator !== "undefined" && /Mac/.test(navigator.userAgent) ? "macos" : typeof navigator !== "undefined" && /Win/.test(navigator.userAgent) ? "windows" : "linux";
const sessionParam = params.get("session");
const sessionKind = os === "linux" && (sessionParam === "x11" || sessionParam === "wayland") ? sessionParam : null;
const portalParam = params.get("portal");
const waylandCancelDetail = "Global Esc cancellation is unavailable on Wayland.";
const shortcutStatus: ShortcutStatus =
  sessionKind !== "wayland"
    ? { backend: "native", dictateTrigger: null, recordTrigger: null, canConfigure: false, holdSupported: true, detail: null, cancelDetail: null }
    : portalParam === "none"
      ? {
          backend: "unavailable", dictateTrigger: null, recordTrigger: null, canConfigure: false, holdSupported: null,
          detail: "org.freedesktop.portal.GlobalShortcuts was not found", cancelDetail: waylandCancelDetail,
        }
      : {
          backend: "portal", dictateTrigger: "Alt+Space", recordTrigger: "Ctrl+Alt+R", canConfigure: true,
          holdSupported: portalParam === "press" ? false : null, detail: null, cancelDetail: waylandCancelDetail,
        };
const noPerms = params.get("perms") === "none";
let perms: Permissions = noPerms
  ? { microphone: "notRequired", systemAudio: "notRequired", accessibility: "notRequired" }
  : { microphone: "granted", systemAudio: "notDetermined", accessibility: "notDetermined" };
// Like the backend: nothing to grant means setup is already done.
let onboarded = params.get("onboarding") !== "0" || noPerms;

const iso = (d: number, h: number, m: number) => {
  const t = new Date(2026, 10, 20 - d, h, m);
  return t.toISOString();
};
const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;

const quickPresets: ShapePreset[] = ["verbatim", "chatMessage", "email", "notesAndLists", "codeAndCommit"];
const tidyShape: Shape = { preset: "tidy", capitals: "sentence", dropFinalPeriod: false, tidy: true,
  spokenPunctuation: true, lineBreaks: "spoken", rewriteWithLocalModel: false };
const presets: Record<ShapePreset, Shape> = {
  tidy: tidyShape,
  verbatim: { ...tidyShape, preset: "verbatim", capitals: "asSaid", tidy: false, spokenPunctuation: false, lineBreaks: "singleLine" },
  chatMessage: { ...tidyShape, preset: "chatMessage", capitals: "relaxed", dropFinalPeriod: true },
  email: { ...tidyShape, preset: "email", lineBreaks: "email" },
  notesAndLists: { ...tidyShape, preset: "notesAndLists", lineBreaks: "notesAndLists" },
  codeAndCommit: { ...tidyShape, preset: "codeAndCommit", capitals: "asSaid", tidy: false, spokenPunctuation: false, lineBreaks: "preserve" },
};
const capabilities: DictationCapabilities = {
  hudVisible: true,
  hudInteractive: os === "macos",
  localRewriteAvailable: false,
  cloudRewrite: "planned",
};
const mockApps: DictationApp[] = [
  { app: { kind: "macosBundleId", value: "com.tinyspeck.slackmacgap" }, displayName: "Slack", suggestedPreset: "chatMessage" },
  { app: { kind: "macosBundleId", value: "com.apple.mail" }, displayName: "Mail", suggestedPreset: "email" },
  { app: { kind: "macosBundleId", value: "com.microsoft.VSCode" }, displayName: "VS Code", suggestedPreset: "codeAndCommit" },
  { app: { kind: "macosBundleId", value: "com.apple.Terminal" }, displayName: "Terminal", suggestedPreset: "verbatim" },
  { app: { kind: "macosBundleId", value: "com.apple.Notes" }, displayName: "Notes", suggestedPreset: "notesAndLists" },
  { app: { kind: "macosBundleId", value: "com.apple.MobileSMS" }, displayName: "Messages", suggestedPreset: null },
];

function sameMatcher(a: AppMatcher, b: AppMatcher) {
  if (a.kind !== b.kind) return false;
  const key = (value: string) => a.kind === "windowsExecutable" ? value.toLowerCase().replace(/\.exe$/, "") : value.toLowerCase();
  return key(a.value) === key(b.value);
}

function preservesIdentifiers(preset: ShapePreset) {
  return preset === "verbatim" || preset === "codeAndCommit";
}

function editedShape(shape: Shape, edits?: ShapeEdits | null): Shape {
  if (preservesIdentifiers(shape.preset)) return shape;
  return { ...shape, capitals: edits?.capitals ?? shape.capitals, dropFinalPeriod: edits?.dropFinalPeriod ?? shape.dropFinalPeriod };
}

const listening: Extract<HudState, { state: "listening" }> = { state: "listening", level: 0.64, handsFree: false, app: "Slack", shape: "chatMessage",
  shortcutPresets: sessionKind === "wayland" ? [] : quickPresets };
const hudFixtures: Record<string, HudState> = {
  listening, "listening-handsfree": { ...listening, handsFree: true },
  "listening-unknown": { ...listening, app: null, shape: "tidy" },
  processing: { state: "processing" },
  inserted: { state: "inserted", words: 23, undoAvailable: false, resultId: null },
  "inserted-no-undo": { state: "inserted", words: 23, undoAvailable: false, resultId: null },
  copied: { state: "copied", words: 23, reason: "You switched apps", reasonCode: "focusChanged" },
  "copied-secure": { state: "copied", words: 23, reason: "Secure input is on", reasonCode: "secureInput" },
  error: { state: "error", message: "Microphone is off", fixAction: "microphoneSettings" },
  "error-transcription": { state: "error", message: "Download a speech model", fixAction: "transcriptionSettings" },
  "error-retry": { state: "error", message: "No speech detected", fixAction: "retry" },
  "error-insertion": { state: "error", message: "Clipboard unavailable", fixAction: "dictationSettings" },
};
let mockHud: HudState = hudFixtures[params.get("hud") ?? state ?? ""] ?? { state: "idle" };
if (!(params.has("hud") || (typeof window !== "undefined" && window.location.hash === "#/hud"))) {
  mockHud = { state: "idle" };
}

let hudTimer: ReturnType<typeof setTimeout> | undefined;
let hudDeadline: number | null = null;
let hudRemaining: number | null = null;
let interactingWithHud = false;
function startHudTimer(duration: number) {
  hudDeadline = Date.now() + duration;
  hudTimer = setTimeout(() => setMockHud({ state: "idle" }), duration);
}
function setMockHud(hud: HudState) {
  clearTimeout(hudTimer);
  hudDeadline = null;
  hudRemaining = null;
  mockHud = hud;
  if (hud.state === "idle" || hud.state === "listening" || hud.state === "processing") interactingWithHud = false;
  if (hud.state === "inserted" || hud.state === "copied") {
    if (interactingWithHud) hudRemaining = 6000;
    else startHudTimer(6000);
  }
  emit("dictation://hud", hud);
}

let settings: AppSettings = {
  appearance: "system", launchAtLogin: true, autoDetectMeetings: true, promptDelaySeconds: 5, stopWhenCallEnds: true,
  includeSystemAudio: true, microphone: null, recordShortcut: "CommandOrControl+Alt+R", transcribeWhere: "local",
  whisperModel: "small", language: null, labelSpeakers: true, liveCaptions: true, echoCancellation: "auto", transportOrder: ["codexCli", "ollama", "grokCli"],
  apiFallbackEnabled: false, summaryLength: "brief", summariesPath: null, codexModel: null, ollamaModel: "llama3.1:8b",
  dictationHotkey: "Alt+Space", dictationMode: "hold", tidyDictation: true, keepDictationAudio: false,
  dictationDefaultShape: null, dictationShapeEdits: {}, dictationStartSound: false,
  dictationAppRules: empty ? [] : mockApps.slice(0, 4).map(({ app, displayName, suggestedPreset }) =>
    ({ app, displayName, preset: suggestedPreset ?? "tidy" })),
  dictationRetentionDays: null, vocabulary: ["Minutae", "whisper.cpp", "Priya Raman", "SwiftUI"], deleteAudioAfterDays: 30, recordingsPath: null,
};

const groups: DayGroup[] = [
  {
    label: "Today",
    items: [
      { id: id(1), title: "Q2 Design Sync", startedAt: iso(0, 14, 30), durationMs: 1934000, status: "done",
        takeaway: "Redesign launch moves to May. Alice owns the updated mockups.", people: ["Alice", "Bob", "Carol"],
        openActions: 2, totalActions: 3, progress: null, failure: null },
      { id: id(2), title: "Hiring loop — Senior iOS", startedAt: iso(0, 11, 0), durationMs: 2882000, status: "processing",
        takeaway: "Transcribing on this Mac…", people: ["Priya", "Sam"], openActions: 0, totalActions: 0,
        progress: { meetingId: id(2), stage: "transcribing", fraction: 0.62, etaSeconds: 120 }, failure: null },
    ],
  },
  {
    label: "Earlier this week",
    items: [
      { id: id(3), title: "Weekly product review", startedAt: iso(1, 10, 0), durationMs: 3280000, status: "done",
        takeaway: "Search ships behind a flag. Onboarding copy needs one more pass.", people: ["Product team · 6"],
        openActions: 5, totalActions: 5, progress: null, failure: null },
      { id: id(4), title: "1:1 with Dana", startedAt: iso(2, 16, 0), durationMs: 1731000, status: "done",
        takeaway: "Career goals for next half; wants to lead the audio pipeline work.", people: ["Dana"],
        openActions: 1, totalActions: 1, progress: null, failure: null },
      { id: id(5), title: "Vendor call — Hosting", startedAt: iso(3, 9, 0), durationMs: 1147000, status: "failed",
        takeaway: null, people: ["Marco", "Lin"], openActions: 0, totalActions: 0, progress: null,
        failure: { kind: "noProviderReady", message: "Codex isn't signed in and Ollama isn't running. Transcript is saved.", fix: "Sign in to Codex" } },
    ],
  },
];

const baseDetail = (meetingId: string): MeetingDetail => ({
  meeting: { id: meetingId, title: "Q2 Design Sync", startedAt: iso(0, 14, 30), endedAt: iso(0, 15, 2), status: "done", sourceApp: "Zoom", failure: null },
  durationMs: 1934000,
  speakers: [
    { id: id(101), meetingId, label: "You", displayName: null, track: "mic" },
    { id: id(102), meetingId, label: "Speaker 1", displayName: "Alice Chen", track: "system" },
    { id: id(103), meetingId, label: "Speaker 2", displayName: "Bob Ortiz", track: "system" },
    { id: id(104), meetingId, label: "Speaker 3", displayName: null, track: "system" },
  ],
  segments: [
    { startMs: 652000, endMs: 660000, text: "If we keep the April date we ship without the new onboarding, and I don't think that's worth it.", track: "system", speakerId: id(102) },
    { startMs: 690000, endMs: 696000, text: "Agreed. Performance budget is still over on the dashboard too.", track: "system", speakerId: id(103) },
    { startMs: 724000, endMs: 731000, text: "So we move to May. Alice, can you share the updated mockups by Friday?", track: "system", speakerId: id(104) },
    { startMs: 761000, endMs: 766000, text: "Yes, Friday works. I'll loop in Bob on the performance numbers.", track: "system", speakerId: id(102) },
    { startMs: 770000, endMs: 774000, text: "Great, I'll move it on the roadmap today.", track: "mic", speakerId: id(101) },
  ],
  summary: {
    overview: "The team agreed to push the redesign launch from April to May so the new onboarding ships with it. Performance on the dashboard is still over budget and needs a pass before beta.",
    points: ["Launch moves to May", "Dashboard performance over budget"],
    decisions: ["Launch moves to the week of 12 May.", "Onboarding is in scope; the referral flow is not.", "Dashboard must load under 1.5s on a cold start."],
    takeaway: "Redesign launch moves to May. Alice owns the updated mockups.",
    markdown: "---\ntitle: \"Q2 Design Sync\"\n---\n\n## Summary\n- Launch moves to May\n", markdownPath: "/Users/you/Library/Minutae/summaries/2026-11-20-q2-design-sync.md",
    provider: "Codex", model: "gpt-6.2-astra",
  },
  actionItems: [
    { id: id(201), text: "Share updated mockups", owner: "Alice", due: "Fri 21 Nov", done: false },
    { id: id(202), text: "Review dashboard performance budget", owner: "Bob", due: null, done: false },
    { id: id(203), text: "Move launch on the roadmap", owner: "Carol", due: null, done: true },
  ],
  moments: [
    { meetingId, atMs: 718000, label: "Decision: May" },
    { meetingId, atMs: 1450000, label: "Perf numbers from Bob" },
  ],
  audio: params.get("audio") === "1" ? { mic: silentWav(), system: silentWav() } : { mic: null, system: null },
  progress: null,
  echoCancel: { ran: true, skipped: null, erleDb: 27.4, delayMs: 38, driftPpm: -4.1, elapsedMs: 410 },
});

/** 2 s of 8 kHz 8-bit mono silence as a data: URL (the browser mock's "file path"). */
let wav: string | null = null;
function silentWav(): string {
  if (wav) return wav;
  const rate = 8000;
  const n = rate * 2;
  const b = new DataView(new ArrayBuffer(44 + n));
  const str = (o: number, t: string) => [...t].forEach((c, i) => b.setUint8(o + i, c.charCodeAt(0)));
  str(0, "RIFF"); b.setUint32(4, 36 + n, true); str(8, "WAVE"); str(12, "fmt ");
  b.setUint32(16, 16, true); b.setUint16(20, 1, true); b.setUint16(22, 1, true); b.setUint32(24, rate, true);
  b.setUint32(28, rate, true); b.setUint16(32, 1, true); b.setUint16(34, 8, true); str(36, "data"); b.setUint32(40, n, true);
  for (let i = 0; i < n; i++) b.setUint8(44 + i, 128);
  let bin = "";
  for (let i = 0; i < b.byteLength; i++) bin += String.fromCharCode(b.getUint8(i));
  wav = `data:audio/wav;base64,${btoa(bin)}`;
  return wav;
}

function detail(meetingId: string): MeetingDetail {
  const d = baseDetail(meetingId);
  if (state === "processing") {
    return {
      ...d,
      meeting: { ...d.meeting, title: "Hiring loop — Senior iOS", status: "processing" },
      summary: null, actionItems: [], moments: [],
      progress: { meetingId, stage: "transcribing", fraction: 0.62, etaSeconds: 120 },
    };
  }
  if (state === "failed") {
    return {
      ...d,
      meeting: {
        ...d.meeting, title: "Vendor call — Hosting", status: "failed",
        failure: { kind: "noProviderReady", message: "Codex isn't signed in and Ollama isn't running. Transcript is saved.", fix: "Sign in to Codex" },
      },
      summary: null, actionItems: [], moments: [],
    };
  }
  return d;
}

const dictationsMode = params.get("dictations");
let dictationsFailedOnce = false;
let dictations: Dictation[] = empty ? [] : [
  ["Thanks for the notes. Let's keep the May date and I'll send the revised plan to the team before Friday's review.", "Mail", 16, 2],
  ["Refactor the recorder so it writes chunks to disk every thirty seconds instead of holding the whole buffer.", "Xcode", 15, 47],
  ["Running ten minutes late, start without me.", "Messages", 11, 20],
  ["Standup: finished the model download flow, today on the summary prompt, no blockers.", "Slack", 9, 5],
].map(([text, app, h, m], i) => ({
  id: id(300 + i), createdAt: iso(params.has("oldDictations") && i === 3 ? 10 : 0, h as number, m as number), text: text as string, rawText: text as string,
  targetApp: app as string,
  appIdentity: { displayName: app as string, macosBundleId: mockApps.find((a) => a.displayName === app)?.app.value ?? null,
    x11WindowClass: null, windowsExecutable: null },
  shape: i === 2 ? null : presets[i === 0 ? "email" : i === 1 ? "codeAndCommit" : "chatMessage"], wordCount: (text as string).split(/\s+/).length, durationMs: 6000, inserted: true,
}));

function removeDictations(remove: (entry: Dictation) => boolean) {
  const remaining = dictations.filter((entry) => !remove(entry));
  if (remaining.length !== dictations.length) {
    dictations = remaining;
    emit("dictations://changed", null);
  }
}

// Live captions. `?captions=off | noModel` sets the status; `?captions=none` keeps them on but
// silent. In `?state=recording` a few lines are already there, then a new line (partial, then
// final) arrives every few seconds while recording.
const captionParam = params.get("captions");
let captionStatus: CaptionStatus = captionParam === "off" ? "off" : captionParam === "noModel" ? "noModel" : "on";
if (captionStatus === "off") settings = { ...settings, liveCaptions: false };
const captionBacklog: CaptionLine[] = [];
let captionId = 100;
function caption(line: Omit<CaptionLine, "id" | "meetingId" | "speaker"> & { id?: number }) {
  const c: CaptionLine = {
    id: line.id ?? captionId++, meetingId: recording.meetingId ?? id(9),
    speaker: line.track === "mic" ? "You" : "Them", ...line,
  };
  captionBacklog.push(c);
  if (captionBacklog.length > 200) captionBacklog.shift();
  emit("recording://caption", c);
}
const CAPTION_SCRIPT: [Track, string, string][] = [
  ["system", "Bob, where did the dashboard numbers", "Bob, where did the dashboard numbers land this week?"],
  ["system", "Cold start is at one point eight", "Cold start is at one point eight seconds, so still over."],
  ["mic", "Okay, let's make that", "Okay, let's make that a blocker for the beta."],
  ["system", "Works for me. I'll write", "Works for me. I'll write up the plan tonight."],
];
let scriptStep = 0;
function startCaptionScript() {
  setInterval(() => {
    if (recording.phase !== "recording" || captionStatus !== "on") return;
    const [track, partial, full] = CAPTION_SCRIPT[scriptStep++ % CAPTION_SCRIPT.length]!;
    const at = recording.elapsedMs;
    const lineId = captionId++;
    caption({ id: lineId, track, atMs: at, endMs: at + 2000, text: partial, final: false });
    setTimeout(() => caption({ id: lineId, track, atMs: at, endMs: at + 4000, text: full, final: true }), 1800);
  }, 6000);
}

let recording: RecordingState = { phase: "idle", meetingId: null, title: null, sourceApp: null, elapsedMs: 0, systemAudio: "on", systemAudioError: null, captions: captionStatus, systemAudioScope: null };
let tick: ReturnType<typeof setInterval> | null = null;
function startTicker() {
  if (tick) return;
  tick = setInterval(() => {
    if (recording.phase !== "recording") return;
    recording = { ...recording, elapsedMs: recording.elapsedMs + 33 };
    const t = recording.elapsedMs / 1000;
    emit("recording://levels", { elapsedMs: recording.elapsedMs, mic: Math.abs(Math.sin(t * 3)) * 0.5, system: Math.abs(Math.sin(t * 1.7 + 1)) * 0.7 });
    if (recording.elapsedMs % 990 < 33) emit("recording://state", recording);
  }, 33);
}
if (params.get("state") === "recording") {
  recording = { phase: "recording", meetingId: id(9), title: "Q2 Design Sync", sourceApp: "Zoom", elapsedMs: 768000, systemAudio: "on", systemAudioError: null, captions: captionStatus, systemAudioScope: "app" };
  startTicker();
  if (captionStatus === "on" && captionParam !== "none") {
    caption({ track: "system", atMs: 724000, endMs: 731000, text: "So we move to May. Alice, can you share the updated mockups by Friday?", final: true });
    caption({ track: "system", atMs: 738000, endMs: 744000, text: "Yes, Friday works. I'll loop in Bob on the performance numbers.", final: true });
    caption({ track: "mic", atMs: 751000, endMs: 755000, text: "Great, I'll move it on the roadmap today.", final: true });
    caption({ track: "system", atMs: 762000, endMs: 766000, text: "One more thing on the onboarding", final: false });
    startCaptionScript();
  }
}

const models: ModelInfo[] = [
  { model: "tiny", sizeBytes: 75e6, state: "missing", fraction: null },
  { model: "base", sizeBytes: 142e6, state: "ready", fraction: null },
  { model: "small", sizeBytes: 466e6, state: "ready", fraction: null },
  { model: "medium", sizeBytes: 1.5e9, state: "downloading", fraction: 0.38 },
];

const providers: ProviderStatus[] = [
  { transport: "codexCli", name: "Codex", availability: "ready", detail: "Uses your ChatGPT sign-in. No API key needed.", model: "gpt-6.2-astra" },
  { transport: "ollama", name: "Ollama · on this Mac", availability: "ready", detail: "Fully offline. llama3.1:8b · slower on long meetings.", model: "llama3.1:8b" },
  { transport: "grokCli", name: "Grok", availability: "notLoggedIn", detail: "Needs an API key.", model: null },
];

const usage: UsageMonth = {
  year: 2026, month: 11, totalUsd: 1.84, summaries: 41, tokens: 1_200_000, freeShare: 0.68,
  byProvider: [
    { provider: "Codex", calls: 28, tokens: 812_000, costUsd: 1.62 },
    { provider: "Ollama · on this Mac", calls: 11, tokens: 344_000, costUsd: 0 },
    { provider: "Whisper · on this Mac", calls: 41, tokens: 0, costUsd: 0 },
    { provider: "Gemini · API fallback", calls: 2, tokens: 41_000, costUsd: 0.22 },
  ],
  byDay: [20, 44, 8, 0, 0, 62, 30, 36, 12, 52, 0, 0, 70, 24, 40, 58, 16, 0, 0, 84].map((v, i) => ({ day: i + 1, costUsd: v / 1000 })),
};

const hits: SearchHit[] = [
  { kind: "meeting", meetingId: id(1), dictationId: null, title: "Q2 Design Sync", snippet: "…so we move to May. Alice, can you share the updated mockups by Friday?", highlights: [], atMs: 718000, date: iso(0, 14, 30) },
  { kind: "meeting", meetingId: id(3), dictationId: null, title: "Weekly product review", snippet: "If onboarding slips, the launch has to move with it.", highlights: [], atMs: 1960000, date: iso(1, 10, 0) },
  { kind: "dictation", meetingId: null, dictationId: id(300), title: "Dictation in Mail", snippet: "Let's keep the May date and I'll send the revised plan…", highlights: [], atMs: null, date: iso(0, 16, 2) },
];

// Import from older Minutae apps. `?import=none` nothing found, `?import=error` the scan fails,
// `?import=running` a run holds at 21 of 54, `?import=failed` the run stops with an error.
const importMode = params.get("import");
let legacyImported = false;
let importing = false;
function legacyScan(): LegacyScan {
  if (importMode === "none") return { sources: [], running: false };
  const sources: LegacySource[] = [
    { kind: "swift", name: "Minutae for Mac", path: "~/Library/Minutae/minutae.db", meetings: 42, dictations: 12,
      alreadyImported: legacyImported ? 53 : 0, audioBytes: legacyImported ? 0 : 3.2e9, error: null },
    { kind: "tauriV1", name: "Minutae 1", path: "~/Library/Application Support/com.nebula.minutae/minutae.db", meetings: 3, dictations: 0,
      alreadyImported: 0, audioBytes: 0, error: "This file isn't a Minutae database, or it is damaged." },
  ];
  return { sources, running: importing };
}
function runImport() {
  const total = 54;
  importing = true;
  let done = 0;
  const step = () => {
    if (importMode === "running" && done >= 21) return;
    if (importMode === "failed" && done >= 8) {
      importing = false;
      emit("import://finished", { result: null, error: "The import stopped unexpectedly. Try again." });
      return;
    }
    done = Math.min(total, done + 7);
    emit("import://progress", { source: "swift", done, total, title: "Q2 Design Sync" });
    if (done < total) {
      setTimeout(step, 60);
      return;
    }
    importing = false;
    legacyImported = true;
    const result: ImportResult = {
      imported: 41, dictations: 12, skipped: 0, failed: 2, copiedBytes: 3.2e9,
      issues: [
        { source: "swift", legacyId: "a1", title: "Vendor call", reason: "Imported without audio: the file is missing.", failed: false },
        { source: "swift", legacyId: "b2", title: "Standup", reason: "It has no readable start time.", failed: true },
        { source: "tauriV1", legacyId: "", title: null, reason: "This file isn't a Minutae database, or it is damaged.", failed: true },
      ],
    };
    emit("import://finished", { result, error: null });
    emit("meetings://changed", { meetingId: null });
    emit("dictations://changed", null);
  };
  setTimeout(step, 60);
}

const STOP = new Set(["the", "and", "did", "does", "was", "were", "why", "what", "when", "who", "how", "for", "with", "that", "this", "are", "you", "our", "its"]);

/** [start, end) ranges of every query word (≥ 3 letters, not a stop word) in `text`, case-insensitive, merged. */
export function highlightRanges(text: string, query: string): [number, number][] {
  const words = query.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter((w) => w.length >= 3 && !STOP.has(w));
  const lower = text.toLowerCase();
  const ranges: [number, number][] = [];
  for (const w of new Set(words)) {
    for (let i = lower.indexOf(w); i !== -1; i = lower.indexOf(w, i + w.length)) ranges.push([i, i + w.length]);
  }
  ranges.sort((a, b) => a[0] - b[0]);
  const merged: [number, number][] = [];
  for (const r of ranges) {
    const last = merged[merged.length - 1];
    if (last && r[0] <= last[1]) last[1] = Math.max(last[1], r[1]);
    else merged.push([r[0], r[1]]);
  }
  return merged;
}

const hitsFor = (query: string) => hits
  .filter((h) => !h.dictationId || dictations.some((entry) => entry.id === h.dictationId))
  .map((h) => ({ ...h, highlights: highlightRanges(h.snippet, query) }));

const delay = <T,>(v: T, ms = 120) => new Promise<T>((r) => setTimeout(() => r(structuredClone(v)), ms));
const fail = (message: string, ms = 120) => new Promise<never>((_, reject) => setTimeout(() => reject(message), ms));

// Global shortcut checks, worded like the backend's update_settings refusals.
const MODS = new Set(["commandorcontrol", "cmdorctrl", "command", "cmd", "super", "meta", "control", "ctrl", "alt", "option", "shift"]);
function normShortcut(acc: string): string {
  return acc
    .split("+")
    .map((k) => k.trim().toLowerCase())
    .map((k) => (k === "commandorcontrol" || k === "cmdorctrl" ? (os === "macos" ? "cmd" : "ctrl") : k === "command" || k === "meta" ? "cmd" : k === "control" ? "ctrl" : k === "option" ? "alt" : k))
    .sort()
    .join("+");
}
function shortcutRefusal(patch: Partial<AppSettings>): string | null {
  const next = { ...settings, ...patch };
  for (const field of ["dictationHotkey", "recordShortcut"] as const) {
    const acc = patch[field];
    if (acc === undefined) continue;
    const parts = acc.split("+").map((k) => k.trim());
    const key = parts.find((k) => !MODS.has(k.toLowerCase()));
    const mods = parts.filter((k) => MODS.has(k.toLowerCase()) && k.toLowerCase() !== "shift");
    if (!key) return `Invalid shortcut "${acc}": it needs a key.`;
    if (mods.length === 0 && !/^F([1-9]|1\d|2[0-4])$/.test(key)) return `"${acc}" needs a modifier key such as Ctrl, Alt or Cmd.`;
  }
  if (normShortcut(next.dictationHotkey) === normShortcut(next.recordShortcut)) return "The dictation and record shortcuts must be different.";
  // Registration can still fail when the OS owns the combination.
  for (const acc of [patch.dictationHotkey, patch.recordShortcut]) {
    if (acc && normShortcut(acc) === "alt+f4") return `Couldn't register ${acc}. The system already uses it.`;
  }
  return null;
}

const ANSWERS: { match: RegExp; text: string; ms: number }[] = [
  { match: /launch|may/i, ms: 900, text: "On Thursday 20 Nov in the Q2 Design Sync. Carol proposed May so onboarding could ship with the redesign; Alice and Bob agreed." },
  { match: /budget|perf|dashboard/i, ms: 1600, text: "Bob owns the dashboard performance budget. He is sharing the numbers with Alice before beta." },
];
function answerFor(question: string): Promise<Answer> {
  const a = ANSWERS.find((x) => x.match.test(question));
  const sources = hitsFor(question).slice(0, 2);
  if (!a) return delay({ text: "Nothing in your library answers that yet.", sources: [], provider: "Codex" }, 600);
  return delay({ text: a.text, sources, provider: "Codex" }, a.ms);
}

// Updater. A check finds nothing unless ?update=available; install "downloads" for ~1.5 s,
// then (instead of restarting) goes back to idle on the new version.
// ?channel=scoop|source: a copy that can't install itself and shows its update command.
const updateParam = params.get("update");
const channelParam = params.get("channel");
let update: UpdateStatus = {
  phase: "idle", currentVersion: "0.4.0", version: null, notes: null, fraction: null, error: null,
  channel: "direct", canInstall: true, updateCommand: null,
  ...(channelParam === "scoop" ? { channel: "scoop", canInstall: false, updateCommand: "scoop update minutae" } as const : {}),
  ...(channelParam === "source"
    ? { channel: "source", canInstall: false, updateCommand: "git pull && ./scripts/install.sh --from-source" } as const
    : {}),
  ...(updateParam === "available" ? { phase: "available", version: "0.5.0", notes: "Faster transcription on Apple silicon." } as const : {}),
  ...(updateParam === "checking" ? { phase: "checking" } as const : {}),
  ...(updateParam === "disabled" ? { phase: "disabled" } as const : {}),
  ...(updateParam === "error"
    ? { phase: "error", error: "Couldn't reach the update server. Check your connection and try again." } as const
    : {}),
};
const setUpdate = (patch: Partial<UpdateStatus>) => {
  update = { ...update, ...patch };
  emit("update://status", update);
};
function mockCheck() {
  setUpdate({ phase: "checking", error: null, fraction: null });
  setTimeout(() => setUpdate(updateParam === "available" ? { phase: "available", version: "0.5.0" } : { phase: "upToDate" }), 700);
}
function mockInstall() {
  setUpdate({ phase: "installing", fraction: 0 });
  let f = 0;
  const t = setInterval(() => {
    f = Math.min(1, f + 0.2);
    setUpdate({ fraction: f });
    if (f >= 1) {
      clearInterval(t);
      setUpdate({ phase: "idle", currentVersion: update.version ?? update.currentVersion, version: null, notes: null, fraction: null });
    }
  }, 300);
}

export async function mockInvoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const r = (v: unknown) => delay(v) as Promise<T>;
  switch (cmd) {
    case "platform": return r({ os, sessionKind, appVersion: "0.4.0-dev" });
    case "shortcut_status": return r(shortcutStatus);
    case "configure_shortcuts": return r(undefined);
    case "get_settings": return r(settings);
    case "update_settings": {
      const patch = args.patch as Partial<AppSettings>;
      const refusal = shortcutRefusal(patch);
      if (refusal) return fail(refusal);
      const retention = patch.dictationRetentionDays;
      if (retention != null && ![7, 30, 90].includes(retention)) {
        return fail("Choose Forever, 7, 30, or 90 days for dictation history.");
      }
      const retentionChanged = retention !== undefined && retention !== settings.dictationRetentionDays;
      settings = { ...settings, ...patch };
      settings.dictationShapeEdits = Object.fromEntries(Object.entries(settings.dictationShapeEdits)
        .filter(([preset]) => !preservesIdentifiers(preset as ShapePreset)));
      if (patch.tidyDictation !== undefined && patch.dictationDefaultShape === undefined) settings.dictationDefaultShape = null;
      emit("settings://changed", settings);
      if (retentionChanged && retention != null) {
        const cutoff = Date.now() - retention * 86_400_000;
        removeDictations((entry) => Date.parse(entry.createdAt) < cutoff);
      }
      if (patch.liveCaptions !== undefined && captionStatus !== "noModel") {
        captionStatus = patch.liveCaptions ? "on" : "off";
        if (recording.phase !== "idle") {
          recording = { ...recording, captions: captionStatus };
          emit("recording://state", recording);
        }
      }
      return r(settings);
    }
    case "permissions": return r(perms);
    case "request_permission": {
      // The mic prompt answers; system audio and accessibility stay notDetermined until relaunch.
      const kind = args.kind as keyof Permissions;
      if (kind === "microphone" && perms.microphone === "notDetermined") perms = { ...perms, microphone: "granted" };
      return r(perms);
    }
    case "onboarding_done": return r(onboarded);
    case "finish_onboarding": onboarded = true; return r(undefined);
    case "list_meetings": return r(empty ? [] : groups);
    case "week_stats": return r(empty ? { recordedMs: 0, openActions: 0 } : { recordedMs: 22320000, openActions: 9 });
    case "get_meeting": return r(detail(args.id as string));
    case "summary_markdown": return r(detail(args.id as string).summary?.markdown ?? "");
    case "recording_state": return r(recording);
    case "start_recording":
    case "accept_call_prompt":
      captionBacklog.length = 0;
      recording = { phase: "recording", meetingId: id(9), title: (args.title as string) ?? "Untitled meeting", sourceApp: (args.sourceApp as string) ?? null, elapsedMs: 0, systemAudio: "on", systemAudioError: null, captions: captionStatus, systemAudioScope: "all" };
      startTicker(); emit("recording://state", recording);
      return r({ id: id(9), title: recording.title, startedAt: new Date().toISOString(), endedAt: null, status: "recording", sourceApp: recording.sourceApp, failure: null });
    case "stop_recording": {
      const meetingId = recording.meetingId ?? id(9);
      recording = { ...recording, phase: "stopping" };
      emit("recording://state", recording);
      await new Promise((done) => setTimeout(done, 600));
      recording = { phase: "idle", meetingId: null, title: null, sourceApp: null, elapsedMs: 0, systemAudio: "on", systemAudioError: null, captions: captionStatus, systemAudioScope: null };
      emit("recording://state", recording);
      return r(meetingId);
    }
    case "pause_recording": recording = { ...recording, phase: "paused" }; emit("recording://state", recording); return r(undefined);
    case "resume_recording": recording = { ...recording, phase: "recording" }; emit("recording://state", recording); return r(undefined);
    case "mark_moment": return r({ meetingId: recording.meetingId, atMs: recording.elapsedMs, label: (args.label as string) ?? "Moment" });
    case "list_input_devices": return r([{ id: "default", name: "MacBook Pro Microphone", isDefault: true }, { id: "airpods", name: "AirPods Pro", isDefault: false }, { id: "mv7", name: "Shure MV7", isDefault: false }]);
    case "dictation_shape_options": {
      const preset = settings.dictationDefaultShape ?? "tidy";
      const defaultShape = settings.dictationDefaultShape === null && !settings.tidyDictation
        ? { ...tidyShape, tidy: false, capitals: "asSaid" as const }
        : editedShape(presets[preset], settings.dictationShapeEdits[preset]);
      const options: DictationShapeOptions = {
        presets: Object.values(presets), quickPresets, capabilities, defaultShape,
      };
      return r(options);
    }
    case "list_dictation_app_rules": return r(settings.dictationAppRules);
    case "list_dictation_apps": return r(mockApps.filter((candidate) =>
      !settings.dictationAppRules.some(({ app }) => sameMatcher(candidate.app, app))));
    case "save_dictation_app_rule": {
      const { app, preset, displayName } = args.rule as DictationAppRule;
      if (!app.value.trim()) return fail("Choose an app for the dictation rule.");
      const rule = { app, preset, displayName: displayName.trim() ? displayName : app.value };
      const found = settings.dictationAppRules.some((r) => sameMatcher(r.app, app));
      settings = { ...settings, dictationAppRules: found
        ? settings.dictationAppRules.map((r) => sameMatcher(r.app, app) ? rule : r)
        : [...settings.dictationAppRules, rule] };
      emit("settings://changed", settings);
      return r(settings);
    }
    case "remove_dictation_app_rule": {
      const matcher = args.matcher as AppMatcher;
      settings = { ...settings, dictationAppRules: settings.dictationAppRules.filter(({ app }) => !sameMatcher(matcher, app)) };
      emit("settings://changed", settings);
      return r(settings);
    }
    case "set_dictation_shape_edits": {
      const preset = args.preset as ShapePreset;
      const edits = args.edits as ShapeEdits | null;
      const dictationShapeEdits = { ...settings.dictationShapeEdits };
      if (edits && !preservesIdentifiers(preset)) dictationShapeEdits[preset] = edits;
      else delete dictationShapeEdits[preset];
      settings = { ...settings, dictationShapeEdits };
      emit("settings://changed", settings);
      return r(settings);
    }
    case "preview_dictation_shape": {
      // Fixed UI fixtures. Arbitrary sample processing is owned by Rust.
      const sample = args.sample as string;
      if (new TextEncoder().encode(sample).length > 16_384) return fail("Use a shorter dictation sample.");
      const preset = args.preset as ShapePreset;
      const shape = editedShape(presets[preset], (args.edits as ShapeEdits | null) ?? settings.dictationShapeEdits[preset]);
      if (sample !== "um so sounds good comma I'll I'll ship it after lunch period" || !shape.tidy) return r(sample);
      const text = shape.capitals === "sentence" ? "Sounds good, I'll ship it after lunch" : "sounds good, I'll ship it after lunch";
      return r(shape.dropFinalPeriod ? text : text + ".");
    }
    case "get_dictation_hud": return r(mockHud);
    case "switch_dictation_shape": {
      const preset = args.preset as ShapePreset;
      if (mockHud.state !== "listening" || !quickPresets.includes(preset)) return fail("Choose a shape while listening.");
      setMockHud({ ...mockHud, shape: preset });
      return r(undefined);
    }
    case "set_dictation_hud_interaction": {
      const active = Boolean(args.hovered || args.focused);
      if (active !== interactingWithHud) {
        interactingWithHud = active;
        if (active && hudDeadline !== null) {
          clearTimeout(hudTimer);
          hudRemaining = Math.max(0, hudDeadline - Date.now());
          hudDeadline = null;
        } else if (!active && hudRemaining !== null) {
          startHudTimer(hudRemaining);
          hudRemaining = null;
        }
      }
      return r(undefined);
    }
    case "dismiss_dictation_hud": setMockHud({ state: "idle" }); return r(undefined);
    case "dismiss_dictation_error":
      if (mockHud.state === "error") setMockHud({ state: "idle" });
      return r(undefined);
    case "undo_dictation_insert":
      if (mockHud.state !== "inserted" || !mockHud.undoAvailable || mockHud.resultId !== args.resultId) return fail("Undo is unavailable.");
      setMockHud({ state: "idle" });
      return r(undefined);
    case "fix_dictation_error":
      if (mockHud.state !== "error") return fail("There is no dictation error to fix.");
      if (mockHud.fixAction === "retry") setMockHud({ ...listening, handsFree: true });
      return r(undefined);
    case "list_dictations":
      if (dictationsMode === "loading") return new Promise<T>(() => {});
      if (dictationsMode === "error") return fail("Couldn't read your dictation history.");
      if (dictationsMode === "errorOnce" && !dictationsFailedOnce) {
        return new Promise<T>((_, reject) =>
          setTimeout(() => {
            dictationsFailedOnce = true;
            reject("Couldn't read your dictation history.");
          }, 120),
        );
      }
      return r(dictations.slice(0, (args.limit as number | undefined) ?? 100));
    case "delete_dictation":
      removeDictations((entry) => entry.id === args.id);
      return r(undefined);
    case "delete_all_dictations":
      removeDictations(() => true);
      return r(undefined);
    case "dictation_stats": {
      const weekStart = new Date();
      weekStart.setDate(weekStart.getDate() - (weekStart.getDay() + 6) % 7);
      weekStart.setHours(0, 0, 0, 0);
      const week = dictations.filter((entry) => Date.parse(entry.createdAt) >= weekStart.getTime());
      const wordsThisWeek = week.reduce((total, entry) => total + entry.wordCount, 0);
      const spokenMs = week.reduce((total, entry) => total + entry.durationMs, 0);
      return r({ wordsThisWeek, minutesSaved: Math.round(Math.max(0, wordsThisWeek / 40 - spokenMs / 60_000)) });
    }
    case "search": return r(!empty && (args.query as string)?.trim() ? hitsFor(args.query as string) : []);
    case "ask": return answerFor(args.question as string) as Promise<T>;
    case "list_models": return r(models);
    case "diarize_model": return r({ state: "ready", fraction: null, sizeBytes: 35e6 });
    case "provider_statuses": return r(providers);
    case "api_key_hint": return r(args.provider === "gemini" ? "AIza••••••••3kQ" : null);
    case "usage_month": return r(usage);
    case "storage_usage": return r({ recordingsBytes: 4.1e9, modelsBytes: 608e6, indexBytes: 46e6, dataDir: "~/Library/Minutae" });
    case "pick_folder": case "export_all": return r(null);
    case "import_legacy_scan": return importMode === "error" ? fail("Couldn't read the older Minutae folder.") : r(legacyScan());
    case "import_legacy_run":
      if (importing) return fail("An import is already running.");
      runImport();
      return r(undefined);
    case "update_status": return r(update);
    case "check_for_updates":
      if (update.phase !== "disabled" && update.phase !== "checking" && update.phase !== "installing") mockCheck();
      return r(update);
    case "install_update":
      if (recording.phase !== "idle") return fail("Stop the recording before installing the update.");
      if (!update.canInstall) return fail(`Update this copy from a terminal: ${update.updateCommand}`);
      if (update.phase !== "available") return fail("No update is ready to install. Check for updates first.");
      mockInstall();
      return r(update);
    // Window plumbing (menu-bar panel): nothing to do in the browser.
    case "open_main": case "quit_app": case "tray_panel_hide": case "tray_panel_resize": return r(undefined);
    default: return r(undefined);
  }
}

export async function mockListen<T>(event: string, cb: (payload: T) => void): Promise<() => void> {
  const set = listeners.get(event) ?? new Set<Listener>();
  set.add(cb as Listener);
  listeners.set(event, set);
  // Captions already sent this recording reach late subscribers too (the app keeps them
  // in one app-wide store, so in the real app nothing is missed after startup).
  if (event === "dictation://hud" && mockHud.state !== "idle") {
    (cb as Listener)(mockHud);
    if ((mockHud.state === "inserted" || mockHud.state === "copied") && hudDeadline === null && hudRemaining === null) startHudTimer(6000);
  }
  if (event === "recording://caption") for (const c of captionBacklog) (cb as Listener)(c);
  return () => set.delete(cb as Listener);
}

/** Test hook: fire a backend event from the console / tests. */
export const mockEmit = (event: string, payload: unknown) => {
  if (event === "dictation://hud") setMockHud(payload as HudState);
  else emit(event, payload);
};
if (typeof window !== "undefined") (window as unknown as { __minutaeMockEmit: typeof emit }).__minutaeMockEmit = mockEmit;
