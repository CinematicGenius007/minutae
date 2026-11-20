/**
 * Navigation helpers on top of the hash router (src/lib/hashLocation.ts).
 *
 *   const go = useGo();  go("/meetings/abc");
 *   openSearch(go)       // → #/library?focus=1 and fires the `minutae:focus-search` window event
 *
 * Query params live inside the hash, so any `go()` to another route drops them; nothing
 * stale (`?q=`, `?focus=1`) survives a later visit. The Library screen reads them with
 * wouter's `useSearch()`, or listens for `FOCUS_SEARCH_EVENT` when it is already mounted.
 */
import { useCallback } from "react";
import { useLocation } from "wouter";

export const FOCUS_SEARCH_EVENT = "minutae:focus-search";

export function useGo() {
  const [, navigate] = useLocation();
  return useCallback((to: string, opts?: { replace?: boolean }) => navigate(to, opts), [navigate]);
}

export function openSearch(go: (to: string) => void, query?: string) {
  const q = query?.trim() ? `&q=${encodeURIComponent(query.trim())}` : "";
  go(`/library?focus=1${q}`);
  window.dispatchEvent(new CustomEvent(FOCUS_SEARCH_EVENT, { detail: { query: query ?? "" } }));
}
