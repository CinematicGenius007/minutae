/**
 * useFailureFix() — what a failed meeting's fix button does, shared by Meetings rows and the
 * Detail failed panel so both send the user to the same place.
 *
 *   const fix = useFailureFix();
 *   fix(failure?.kind, () => retry());
 *
 *   modelMissing                         → Settings › Transcription
 *   notInstalled / notLoggedIn /
 *   noProviderReady / unavailable        → Settings › Summaries
 *   permissionDenied                     → OS privacy settings (microphone)
 *   anything else                        → retry
 */
import { useCallback } from "react";
import { useToast } from "@minutae/ui";
import { api } from "../lib/api";
import type { FailureKind } from "../lib/types";
import { errorText } from "./useLoad";
import { useGo } from "./useNav";

export function useFailureFix() {
  const go = useGo();
  const toast = useToast();
  return useCallback(
    (kind: FailureKind | null | undefined, retry: () => void) => {
      switch (kind) {
        case "permissionDenied":
          api.openSystemSettings("microphone").catch((e) => toast.show({ title: `Couldn't open settings. ${errorText(e)}` }));
          return;
        case "modelMissing":
          go("/settings/transcription");
          return;
        case "notInstalled":
        case "notLoggedIn":
        case "noProviderReady":
        case "unavailable":
          go("/settings/summaries");
          return;
        default:
          retry();
      }
    },
    [go, toast],
  );
}
