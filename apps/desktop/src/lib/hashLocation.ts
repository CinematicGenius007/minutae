/**
 * Hash routing that keeps the query string inside the hash: `#/library?q=launch&focus=1`.
 *
 * wouter's stock `useHashLocation` writes `?…` to `location.search` (before the hash), where it
 * outlives the route: a later `#/library` visit would still see the old `?q=`. Here the whole
 * route, query included, lives in the hash, so replacing the hash (links, sidebar, back) drops
 * stale params. App code never reads `location.search` (the mock backend's switches use it).
 *
 *   <Router hook={useHashPath} searchHook={useHashSearch}>   // wouter useSearch() → "q=…"
 */
import { useSyncExternalStore } from "react";

const listeners = new Set<() => void>();
const onHashChange = () => {
  for (const cb of listeners) cb();
};

function subscribe(cb: () => void) {
  if (listeners.size === 0) window.addEventListener("hashchange", onHashChange);
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
    if (listeners.size === 0) window.removeEventListener("hashchange", onHashChange);
  };
}

/** "#/library?q=x" → ["/library", "q=x"]. */
function splitHash(): [string, string] {
  const raw = window.location.hash.replace(/^#?\/?/, "");
  const at = raw.indexOf("?");
  const path = `/${at === -1 ? raw : raw.slice(0, at)}`;
  return [path, at === -1 ? "" : raw.slice(at + 1)];
}

const currentPath = () => splitHash()[0];
const currentSearch = () => splitHash()[1];

export function navigateHash(to: string, { replace = false }: { replace?: boolean; state?: unknown } = {}) {
  const oldURL = window.location.href;
  const url = new URL(oldURL);
  url.hash = `/${to.replace(/^#?\/?/, "")}`;
  const newURL = url.href;
  if (newURL === oldURL) return;
  window.history[replace ? "replaceState" : "pushState"](null, "", newURL);
  window.dispatchEvent(new HashChangeEvent("hashchange", { oldURL, newURL }));
}

export function useHashPath(): [string, typeof navigateHash] {
  return [useSyncExternalStore(subscribe, currentPath, () => "/"), navigateHash];
}
useHashPath.hrefs = (href: string) => `#${href}`;
useHashPath.searchHook = useHashSearch;

export function useHashSearch(): string {
  return useSyncExternalStore(subscribe, currentSearch, () => "");
}
