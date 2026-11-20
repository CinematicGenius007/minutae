/**
 * useShortcutStatus() — which global shortcut backend is live (native plugin, Wayland
 * portal, or none) and what the portal bound. Follows `shortcuts://changed`.
 *
 *   const status = useShortcutStatus();   // ShortcutStatus | null while loading
 */
import { useEffect, useState } from "react";
import { api, events } from "../lib/api";
import type { ShortcutStatus } from "../lib/types";
import { useEvent } from "./useEvent";

export function useShortcutStatus(): ShortcutStatus | null {
  const [status, setStatus] = useState<ShortcutStatus | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .shortcutStatus()
      .then((s) => alive && setStatus(s))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);
  useEvent(events.shortcutsChanged, setStatus);
  return status;
}
