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

/** Live captions for the meeting being recorded. Partial (non-final) lines are replaced in place. */
export function useCaptions(meetingId: string | null): CaptionLine[] {
  const [lines, setLines] = useState<CaptionLine[]>([]);
  useEffect(() => {
    setLines([]);
    let off: (() => void) | null = null;
    let cancelled = false;
    events.caption((c) => {
      setLines((prev) => {
        const last = prev[prev.length - 1];
        if (last && !last.final && last.speaker === c.speaker) return [...prev.slice(0, -1), c];
        return [...prev, c];
      });
    }).then((u) => (cancelled ? u() : (off = u)));
    return () => {
      cancelled = true;
      off?.();
    };
  }, [meetingId]);
  return lines;
}
