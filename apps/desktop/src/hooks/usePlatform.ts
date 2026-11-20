/**
 * Platform hooks backed by `api.platform()` (falls back to the user agent until it answers).
 *
 *   const platform = usePlatform();       // "mac" | "windows" | "linux" (kit Platform type)
 *   const info = usePlatformInfo();       // PlatformInfo | null  { os, sessionKind, appVersion }
 *
 * The backend is asked once per app session; every caller shares the answer.
 */
import { useEffect, useState } from "react";
import { detectPlatform, type Platform } from "@minutae/ui";
import { api } from "../lib/api";
import type { Os, PlatformInfo } from "../lib/types";

let cached: PlatformInfo | null = null;
let pending: Promise<PlatformInfo> | null = null;

function load(): Promise<PlatformInfo> {
  if (!pending) pending = api.platform().then((p) => (cached = p));
  return pending;
}

export function osToPlatform(os: Os): Platform {
  return os === "macos" ? "mac" : os;
}

export function usePlatformInfo(): PlatformInfo | null {
  const [info, setInfo] = useState<PlatformInfo | null>(cached);
  useEffect(() => {
    if (cached) return;
    let alive = true;
    load().then((p) => alive && setInfo(p)).catch(() => {});
    return () => {
      alive = false;
    };
  }, []);
  return info;
}

export function usePlatform(): Platform {
  const info = usePlatformInfo();
  return info ? osToPlatform(info.os) : detectPlatform();
}
