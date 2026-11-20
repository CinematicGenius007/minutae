import { useMemo } from "react";
import { detectPlatform, type Platform } from "../lib/platform";

/** The host OS, detected once from the user agent. */
export function usePlatform(override?: Platform): Platform {
  return useMemo(() => override ?? detectPlatform(), [override]);
}
