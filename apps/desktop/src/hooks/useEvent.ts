/**
 * useEvent(subscribe, handler, enabled?)
 *
 * Subscribes to one backend event from `events.*` in lib/api.ts for the life of the
 * component. `handler` may change every render (kept in a ref); the subscription is
 * made once per `subscribe` function. Handles the async unlisten race on unmount.
 *
 *   useEvent(events.meetingsChanged, () => refetch());
 */
import { useEffect, useRef } from "react";
import type { Unlisten } from "../lib/api";

export type Subscribe<T> = (cb: (payload: T) => void) => Promise<Unlisten>;

export function useEvent<T>(subscribe: Subscribe<T>, handler: (payload: T) => void, enabled = true): void {
  const ref = useRef(handler);
  ref.current = handler;
  useEffect(() => {
    if (!enabled) return;
    let unlisten: Unlisten | null = null;
    let cancelled = false;
    subscribe((p) => ref.current(p)).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [subscribe, enabled]);
}
