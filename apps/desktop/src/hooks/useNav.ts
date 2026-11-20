/**
 * Navigation helpers on top of wouter's hash location.
 *
 *   const go = useGo();  go("/meetings/abc");
 *   openSearch(go)       // → /library?focus=1 and fires the `minutae:focus-search` window event
 *
 * `go()` clears a stale `?focus=1` left by openSearch so later visits don't re-focus.
 * The Library screen can read `useSearch()` (wouter) for `focus=1`, or listen for
 * `window.addEventListener(FOCUS_SEARCH_EVENT, …)` when it is already mounted.
 */
import { useCallback } from "react";
import { useLocation } from "wouter";

export const FOCUS_SEARCH_EVENT = "minutae:focus-search";

export function useGo() {
  const [, navigate] = useLocation();
  return useCallback(
    (to: string, opts?: { replace?: boolean }) => {
      let target = to;
      if (!to.includes("?") && /(^|&|\?)focus=/.test(window.location.search)) target = `${to}?`;
      navigate(target, opts);
    },
    [navigate],
  );
}

export function openSearch(go: (to: string) => void, query?: string) {
  const q = query?.trim() ? `&q=${encodeURIComponent(query.trim())}` : "";
  go(`/library?focus=1${q}`);
  window.dispatchEvent(new CustomEvent(FOCUS_SEARCH_EVENT, { detail: { query: query ?? "" } }));
}
