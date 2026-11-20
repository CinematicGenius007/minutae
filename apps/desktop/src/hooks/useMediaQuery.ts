/**
 * useMediaQuery(query) → boolean, live. Plus the shared layout breakpoints (DESIGN-LANGUAGE §4):
 *
 *   const narrow = useNarrow();    // < 900 px: no sidebar, bottom tab bar
 *   const compact = useCompact();  // < 1200 px: sidebar 200, rail drops below
 */
import { useSyncExternalStore } from "react";

export function useMediaQuery(query: string): boolean {
  return useSyncExternalStore(
    (cb) => {
      const m = window.matchMedia(query);
      m.addEventListener("change", cb);
      return () => m.removeEventListener("change", cb);
    },
    () => window.matchMedia(query).matches,
    () => false,
  );
}

export const NARROW_QUERY = "(max-width: 899px)";
export const COMPACT_QUERY = "(max-width: 1199px)";
export const useNarrow = () => useMediaQuery(NARROW_QUERY);
export const useCompact = () => useMediaQuery(COMPACT_QUERY);
