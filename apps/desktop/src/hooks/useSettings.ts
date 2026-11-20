/**
 * App settings, shared app-wide (one fetch + one `settings://changed` subscription).
 *
 *   const { settings, error, reload, update } = useSettings();
 *     settings: AppSettings | null (null until loaded)
 *     error: load error text, or null
 *     reload(): retry the initial load
 *     update(patch): Promise<AppSettings> — writes through api.updateSettings, applies
 *       optimistically and rolls back (then rethrows) on failure
 */
import { useSyncExternalStore } from "react";
import { api, events } from "../lib/api";
import type { AppSettings } from "../lib/types";
import { errorText } from "./useLoad";

interface Snapshot {
  settings: AppSettings | null;
  error: string | null;
}

let snap: Snapshot = { settings: null, error: null };
const subs = new Set<() => void>();
let started = false;

function publish(next: Partial<Snapshot>) {
  snap = { ...snap, ...next };
  for (const cb of subs) cb();
}

const set = (s: AppSettings) => publish({ settings: s, error: null });

function load() {
  api
    .getSettings()
    .then(set)
    .catch((e) => publish({ error: errorText(e) }));
}

export function reloadSettings() {
  publish({ error: null });
  load();
}

function subscribe(cb: () => void) {
  if (!started) {
    started = true;
    void events.settingsChanged(set);
    load();
  }
  subs.add(cb);
  return () => subs.delete(cb);
}

export async function updateSettings(patch: Partial<AppSettings>): Promise<AppSettings> {
  const prev = snap.settings;
  if (prev) set({ ...prev, ...patch });
  try {
    const next = await api.updateSettings(patch);
    set(next);
    return next;
  } catch (e) {
    if (prev) set(prev);
    throw e;
  }
}

export function useSettings() {
  const { settings, error } = useSyncExternalStore(subscribe, () => snap, () => snap);
  return { settings, error, reload: reloadSettings, update: updateSettings };
}
