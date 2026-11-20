// Dictation HUD window (label "hud"). Transparent, never takes focus; shows the kit HudPill.
// The listening timer is measured here from the first "listening" event of a hold.
import { HudPill } from "@minutae/ui";
import { useEffect, useRef, useState } from "react";
import { events } from "../../lib/api";
import type { HudState } from "../../lib/types";
import s from "./overlays.module.css";
import { useTransparentWindow } from "./useTransparentWindow";

const BARS = 6;

export default function HudWindow() {
  useTransparentWindow();
  const [hud, setHud] = useState<HudState>({ state: "idle" });
  const [levels, setLevels] = useState<number[]>(() => Array(BARS).fill(0.15));
  const [elapsedMs, setElapsedMs] = useState(0);
  const startedAt = useRef<number | null>(null);
  const [subscribed, setSubscribed] = useState(false);

  useEffect(() => {
    let off: (() => void) | null = null;
    let dead = false;
    void events
      .hud((next) => {
        setHud(next);
        if (next.state === "listening") {
          if (startedAt.current === null) {
            startedAt.current = Date.now();
            setElapsedMs(0);
          }
          // Rolling window of the most recent levels, newest last.
          setLevels((prev) => [...prev.slice(1 - BARS), Math.max(0.12, Math.min(1, next.level))]);
        } else {
          startedAt.current = null;
          setLevels(Array(BARS).fill(0.15));
        }
      })
      .then((u) => {
        if (dead) u();
        else {
          off = u;
          setSubscribed(true);
        }
      });
    return () => {
      dead = true;
      off?.();
    };
  }, []);

  const listening = hud.state === "listening";
  useEffect(() => {
    if (!listening) return;
    const t = setInterval(() => {
      if (startedAt.current !== null) setElapsedMs(Date.now() - startedAt.current);
    }, 250);
    return () => clearInterval(t);
  }, [listening]);

  return (
    <div className={s.hudStage} data-screen="hud" data-ready={String(subscribed)}>
      {hud.state === "idle" ? null : hud.state === "listening" ? (
        <HudPill
          state="listening"
          levels={levels}
          elapsedMs={elapsedMs}
          message={hud.handsFree ? "Listening hands-free" : "Listening"}
        />
      ) : hud.state === "processing" ? (
        <HudPill state="processing" />
      ) : hud.state === "inserted" ? (
        <HudPill state="inserted" words={hud.words} />
      ) : hud.state === "copied" ? (
        <HudPill state="copied" message={hud.reason || "No text field · copied instead"} />
      ) : (
        <HudPill state="error" message={hud.message} />
      )}
    </div>
  );
}
