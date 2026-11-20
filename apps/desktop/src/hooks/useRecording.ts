/**
 * Recording state, shared app-wide (one backend subscription for every caller).
 *
 *   const { state, elapsedMs, live, paused } = useRecording();
 *     state:     RecordingState | null (null until the backend answers)
 *     elapsedMs: interpolated between backend ticks so timers advance smoothly at 1 Hz
 *     live:      phase is recording | paused | stopping (a meeting is open)
 *     capturing: phase === "recording" (audio is being written; red is allowed)
 *
 *   useLevels((frame) => …)     // ~30 Hz LevelFrame callback; does NOT re-render
 *   useCaptions()               // CaptionLine[] for the current recording (live transcript)
 */
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { api, events } from "../lib/api";
import type { CaptionLine, LevelFrame, RecordingState } from "../lib/types";

interface Snapshot { state: RecordingState | null; at: number }

let snap: Snapshot = { state: null, at: 0 };
const subs = new Set<() => void>();
let started = false;

function set(state: RecordingState) {
  snap = { state, at: performance.now() };
  for (const s of subs) s();
}

function ensureStarted() {
  if (started) return;
  started = true;
  void events.recordingState(set);
  api.recordingState().then((s) => {
    // Don't clobber a newer event that arrived first.
    if (!snap.state) set(s);
  }).catch(() => {});
}

function subscribe(cb: () => void) {
  ensureStarted();
  subs.add(cb);
  return () => subs.delete(cb);
}

/** Push a state locally (e.g. right after a command resolves) before the event lands. */
export function setRecordingState(s: RecordingState) {
  set(s);
}

export function useRecording() {
  const { state, at } = useSyncExternalStore(subscribe, () => snap, () => snap);
  const capturing = state?.phase === "recording";
  const [now, setNow] = useState(() => performance.now());
  useEffect(() => {
    if (!capturing) return;
    const id = setInterval(() => setNow(performance.now()), 250);
    return () => clearInterval(id);
  }, [capturing]);
  const elapsedMs = state ? state.elapsedMs + (capturing ? Math.max(0, now - at) : 0) : 0;
  const live = !!state && state.phase !== "idle";
  return { state, elapsedMs, live, capturing, paused: state?.phase === "paused" };
}

export function useLevels(onFrame: (f: LevelFrame) => void, enabled = true) {
  const ref = useRef(onFrame);
  ref.current = onFrame;
  useEffect(() => {
    if (!enabled) return;
    let off: (() => void) | null = null;
    let cancelled = false;
    events.levels((f) => ref.current(f)).then((u) => (cancelled ? u() : (off = u)));
    return () => {
      cancelled = true;
      off?.();
    };
  }, [enabled]);
}

// Live captions: one app-wide subscription (started with the first `useCaptions`), so lines
// survive leaving and reopening the Live screen. Only the latest meeting's lines are kept.
const MAX_CAPTION_LINES = 200;
const NO_CAPTIONS: CaptionLine[] = [];
let captions: { meetingId: string | null; lines: CaptionLine[] } = { meetingId: null, lines: NO_CAPTIONS };
const captionSubs = new Set<() => void>();
let captionsStarted = false;

/**
 * Apply one `recording://caption` event: same `id` replaces (partial → final), a final with
 * empty text removes, anything else is inserted in time order. Keeps the last 200 lines.
 */
export function applyCaption(lines: CaptionLine[], c: CaptionLine): CaptionLine[] {
  const i = lines.findIndex((l) => l.id === c.id);
  if (c.final && !c.text.trim()) return i < 0 ? lines : lines.filter((_, j) => j !== i);
  if (i >= 0) return lines.map((l, j) => (j === i ? c : l));
  let at = lines.length;
  while (at > 0 && lines[at - 1]!.atMs > c.atMs) at--;
  const next = [...lines.slice(0, at), c, ...lines.slice(at)];
  return next.length > MAX_CAPTION_LINES ? next.slice(-MAX_CAPTION_LINES) : next;
}

function subscribeCaptions(cb: () => void) {
  if (!captionsStarted) {
    captionsStarted = true;
    void events.caption((c) => {
      const base = captions.meetingId === c.meetingId ? captions.lines : NO_CAPTIONS;
      captions = { meetingId: c.meetingId, lines: applyCaption(base, c) };
      for (const s of captionSubs) s();
    });
  }
  captionSubs.add(cb);
  return () => captionSubs.delete(cb);
}

/** Live captions for the meeting being recorded (time order; partial lines replaced in place). */
export function useCaptions(meetingId: string | null): CaptionLine[] {
  const snapshot = useSyncExternalStore(subscribeCaptions, () => captions, () => captions);
  return meetingId && snapshot.meetingId === meetingId ? snapshot.lines : NO_CAPTIONS;
}
