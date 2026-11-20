/**
 * useLoad(fn, deps) — runs `fn` on mount and whenever `deps` change.
 *
 *   const [state, reload, setState] = useLoad(() => api.listDictations());
 *     state: { status: "loading" | "ready" | "error", data, error }
 *     reload(): refetch without flashing the loading state when data is already on screen
 *
 * Stale responses are dropped. `errorText(e)` turns any thrown value into one sentence.
 */
import { useCallback, useEffect, useRef, useState } from "react";

export type Load<T> =
  | { status: "loading"; data: null; error: null }
  | { status: "ready"; data: T; error: null }
  | { status: "error"; data: T | null; error: string };

export function errorText(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return "Something went wrong.";
}

export function useLoad<T>(fn: () => Promise<T>, deps: unknown[] = []) {
  const [state, setState] = useState<Load<T>>({ status: "loading", data: null, error: null });
  const fnRef = useRef(fn);
  fnRef.current = fn;
  const seq = useRef(0);

  const reload = useCallback(() => {
    const n = ++seq.current;
    return fnRef
      .current()
      .then((data) => {
        if (n === seq.current) setState({ status: "ready", data, error: null });
      })
      .catch((e) => {
        if (n === seq.current) setState((s) => ({ status: "error", data: s.data, error: errorText(e) }));
      });
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: deps are passed by the caller
  useEffect(() => {
    setState((s) => (s.data === null ? { status: "loading", data: null, error: null } : s));
    void reload();
  }, deps);

  return [state, reload, setState] as const;
}
