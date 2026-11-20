/**
 * Record / stop with the app's standard follow-up navigation and error toasts.
 *
 *   const { start, stop, toggle, busy } = useRecordActions();
 *   start(opts?)  → api.startRecording, then /live
 *   stop()        → api.stopRecording, then /meetings/:id
 *   toggle()      → start when idle, stop when live (⌘R)
 */
import { useCallback, useState } from "react";
import { useToast } from "@minutae/ui";
import { api } from "../lib/api";
import { toError } from "./useQuery";
import { useGo } from "./useNav";
import { useRecording } from "./useRecording";

export function useRecordActions() {
  const go = useGo();
  const toast = useToast();
  const { live } = useRecording();
  const [busy, setBusy] = useState(false);

  const start = useCallback(
    async (opts?: { title?: string; sourceApp?: string }) => {
      setBusy(true);
      try {
        await api.startRecording(opts);
        go("/live");
      } catch (e) {
        toast.show({ title: `Couldn't start recording. ${toError(e).message}` });
      } finally {
        setBusy(false);
      }
    },
    [go, toast],
  );

  const stop = useCallback(async () => {
    setBusy(true);
    try {
      const id = await api.stopRecording();
      go(`/meetings/${id}`);
    } catch (e) {
      toast.show({ title: `Couldn't stop recording. ${toError(e).message}` });
    } finally {
      setBusy(false);
    }
  }, [go, toast]);

  const toggle = useCallback(() => (live ? stop() : start()), [live, start, stop]);
  return { start, stop, toggle, busy };
}
