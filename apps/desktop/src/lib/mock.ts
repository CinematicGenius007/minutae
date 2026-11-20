// In-browser fake backend. Used when the UI runs outside Tauri (vite dev, screenshot tests).
// Content mirrors the Paper artboards so screens can be compared 1:1.
// URL switches (query string before the hash, e.g. `/?state=failed#/meetings/<id>`):
//   ?state=empty       empty lists (Meetings, Dictate, Library)
//   ?state=recording   start in a live recording
//   ?state=processing  the Detail screen shows a meeting still transcribing (no summary yet)
//   ?state=failed      the Detail screen shows a meeting whose summary failed (reason + fix)
//   ?onboarding=0      first run: onboarding_done() is false, so the shell sends you to /onboarding
//   ?os=macos          report this OS instead of guessing from the user agent (macos | windows | linux)
//   ?perms=none        every permission is notRequired (Windows / Linux): onboarding has nothing to ask
//   ?audio=1           meetings have short silent audio tracks, so the player is enabled
//
// Behaviour mirrors the real backend where the UI must cope with it:
//   - request_permission("systemAudio" | "accessibility") on macOS can keep answering notDetermined
//   - update_settings refuses bare-key and duplicate global shortcuts with an error string
//   - stop_recording reports phase "stopping" for ~600 ms before the meeting is saved
//   - ask() answers take a different time per question, so a later question can land first

import type {
  Answer, AppSettings, DayGroup, Dictation, MeetingDetail, ModelInfo, Permissions, ProviderStatus, RecordingState, SearchHit,
  UsageMonth,
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

let settings: AppSettings = {
  appearance: "system", launchAtLogin: true, autoDetectMeetings: true, promptDelaySeconds: 5, stopWhenCallEnds: true,
  includeSystemAudio: true, microphone: null, recordShortcut: "CommandOrControl+Alt+R", transcribeWhere: "local",
  whisperModel: "small", language: null, labelSpeakers: true, transportOrder: ["codexCli", "ollama", "grokCli"],
  apiFallbackEnabled: false, summaryLength: "brief", summariesPath: null, codexModel: null, ollamaModel: "llama3.1:8b",
  dictationHotkey: "Alt+Space", dictationMode: "hold", tidyDictation: true, keepDictationAudio: false,
  vocabulary: ["Minutae", "whisper.cpp", "Priya Raman", "SwiftUI"], deleteAudioAfterDays: 30, recordingsPath: null,
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

const dictations: Dictation[] = [
  ["Thanks for the notes. Let's keep the May date and I'll send the revised plan to the team before Friday's review.", "Mail", 16, 2],
  ["Refactor the recorder so it writes chunks to disk every thirty seconds instead of holding the whole buffer.", "Xcode", 15, 47],
  ["Running ten minutes late, start without me.", "Messages", 11, 20],
  ["Standup: finished the model download flow, today on the summary prompt, no blockers.", "Slack", 9, 5],
].map(([text, app, h, m], i) => ({
  id: id(300 + i), createdAt: iso(0, h as number, m as number), text: text as string, rawText: text as string,
  targetApp: app as string, wordCount: (text as string).split(/\s+/).length, durationMs: 6000, inserted: true,
}));

let recording: RecordingState = { phase: "idle", meetingId: null, title: null, sourceApp: null, elapsedMs: 0, systemAudio: "on", systemAudioError: null };
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
  recording = { phase: "recording", meetingId: id(9), title: "Q2 Design Sync", sourceApp: "Zoom", elapsedMs: 768000, systemAudio: "on", systemAudioError: null };
  startTicker();
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

const hitsFor = (query: string) => hits.map((h) => ({ ...h, highlights: highlightRanges(h.snippet, query) }));

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

export async function mockInvoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const r = (v: unknown) => delay(v) as Promise<T>;
  switch (cmd) {
    case "platform": return r({ os, sessionKind: null, appVersion: "0.4.0-dev" });
    case "get_settings": return r(settings);
    case "update_settings": {
      const patch = args.patch as Partial<AppSettings>;
      const refusal = shortcutRefusal(patch);
      if (refusal) return fail(refusal);
      settings = { ...settings, ...patch };
      emit("settings://changed", settings);
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
      recording = { phase: "recording", meetingId: id(9), title: (args.title as string) ?? "Untitled meeting", sourceApp: (args.sourceApp as string) ?? null, elapsedMs: 0, systemAudio: "on", systemAudioError: null };
      startTicker(); emit("recording://state", recording);
      return r({ id: id(9), title: recording.title, startedAt: new Date().toISOString(), endedAt: null, status: "recording", sourceApp: recording.sourceApp, failure: null });
    case "stop_recording": {
      const meetingId = recording.meetingId ?? id(9);
      recording = { ...recording, phase: "stopping" };
      emit("recording://state", recording);
      await new Promise((done) => setTimeout(done, 600));
      recording = { phase: "idle", meetingId: null, title: null, sourceApp: null, elapsedMs: 0, systemAudio: "on", systemAudioError: null };
      emit("recording://state", recording);
      return r(meetingId);
    }
    case "pause_recording": recording = { ...recording, phase: "paused" }; emit("recording://state", recording); return r(undefined);
    case "resume_recording": recording = { ...recording, phase: "recording" }; emit("recording://state", recording); return r(undefined);
    case "mark_moment": return r({ meetingId: recording.meetingId, atMs: recording.elapsedMs, label: (args.label as string) ?? "Moment" });
    case "list_input_devices": return r([{ id: "default", name: "MacBook Pro Microphone", isDefault: true }, { id: "airpods", name: "AirPods Pro", isDefault: false }, { id: "mv7", name: "Shure MV7", isDefault: false }]);
    case "list_dictations": return r(empty ? [] : dictations);
    case "dictation_stats": return r({ wordsThisWeek: 4210, minutesSaved: 38 });
    case "search": return r(!empty && (args.query as string)?.trim() ? hitsFor(args.query as string) : []);
    case "ask": return answerFor(args.question as string) as Promise<T>;
    case "list_models": return r(models);
    case "diarize_model": return r({ state: "ready", fraction: null, sizeBytes: 35e6 });
    case "provider_statuses": return r(providers);
    case "api_key_hint": return r(args.provider === "gemini" ? "AIza••••••••3kQ" : null);
    case "usage_month": return r(usage);
    case "storage_usage": return r({ recordingsBytes: 4.1e9, modelsBytes: 608e6, indexBytes: 46e6, dataDir: "~/Library/Minutae" });
    case "pick_folder": case "export_all": return r(null);
    default: return r(undefined);
  }
}

export async function mockListen<T>(event: string, cb: (payload: T) => void): Promise<() => void> {
  const set = listeners.get(event) ?? new Set<Listener>();
  set.add(cb as Listener);
  listeners.set(event, set);
  return () => set.delete(cb as Listener);
}

/** Test hook: fire a backend event from the console / tests. */
export const mockEmit = emit;
if (typeof window !== "undefined") (window as unknown as { __minutaeMockEmit: typeof emit }).__minutaeMockEmit = emit;
