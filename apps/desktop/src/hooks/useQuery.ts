/**
 * useQuery(fetcher, deps, options?) — tiny data hook for `api.*` calls.
 *
 *   const { data, error, loading, refetch, setData } =
 *     useQuery(() => api.listMeetings(), [], { refetchOn: [events.meetingsChanged] });
 *
 * - Keeps the previous `data` while refetching (no layout jump); `loading` is true only
 *   until the first result for the current deps arrives.
 * - `refetchOn`: event subscribers from `events.*`; any payload triggers a quiet refetch.
 *   Pass stable references (the `events.*` functions themselves are stable).
 * - Stale responses (deps changed mid-flight) are dropped.
 * - `setData` allows optimistic updates.
 */
import { type DependencyList, useCallback, useEffect, useRef, useState } from "react";
import type { Unlisten } from "../lib/api";

export interface QueryResult<T> {
  data: T | undefined;
  error: Error | null;
  /** True until the first response for the current deps. */
  loading: boolean;
  refetch: () => Promise<void>;
  setData: (next: T | ((prev: T | undefined) => T | undefined)) => void;
}

export interface QueryOptions {
  // biome-ignore lint/suspicious/noExplicitAny: any event payload triggers a refetch
  refetchOn?: Array<(cb: (payload: any) => void) => Promise<Unlisten>>;
  /** Skip fetching while false. */
  enabled?: boolean;
}

export function toError(e: unknown): Error {
  if (e instanceof Error) return e;
  if (typeof e === "string") return new Error(e);
  if (e && typeof e === "object" && "message" in e) return new Error(String((e as { message: unknown }).message));
  return new Error("Something went wrong");
}

export function useQuery<T>(fetcher: () => Promise<T>, deps: DependencyList, options: QueryOptions = {}): QueryResult<T> {
  const { refetchOn, enabled = true } = options;
  const [data, setData] = useState<T | undefined>(undefined);
  const [error, setError] = useState<Error | null>(null);
  const [loading, setLoading] = useState(enabled);
  const fetcherRef = useRef(fetcher);
  fetcherRef.current = fetcher;
  const seq = useRef(0);

  const run = useCallback(async () => {
    const mine = ++seq.current;
    try {
      const v = await fetcherRef.current();
      if (mine !== seq.current) return;
      setData(v);
      setError(null);
    } catch (e) {
      if (mine !== seq.current) return;
      setError(toError(e));
    } finally {
      if (mine === seq.current) setLoading(false);
    }
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: caller-provided deps
  useEffect(() => {
    if (!enabled) return;
    setLoading(true);
    void run();
  }, [...deps, enabled]);

  useEffect(() => {
    if (!enabled || !refetchOn?.length) return;
    const offs: Unlisten[] = [];
    let cancelled = false;
    for (const sub of refetchOn) {
      sub(() => void run()).then((u) => (cancelled ? u() : offs.push(u)));
    }
    return () => {
      cancelled = true;
      for (const u of offs) u();
    };
    // biome-ignore lint/correctness/useExhaustiveDependencies: subscribers are stable module functions
  }, [enabled, run, ...(refetchOn ?? [])]);

  return { data, error, loading, refetch: run, setData };
}
