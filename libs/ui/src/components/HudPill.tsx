import { Check, CircleAlert } from "lucide-react";
import type { CSSProperties } from "react";
import { cx } from "../lib/cx";
import { formatDuration } from "../lib/format";
import s from "./Hud.module.css";

export type HudState = "listening" | "processing" | "inserted" | "copied" | "error";

export interface HudPillProps {
  state: HudState;
  /** Mic levels 0–1 for the listening bars (most recent last). Six bars are drawn. */
  levels?: number[];
  /** Word count for "inserted". */
  words?: number;
  /** Overrides the default copy for the state. For "error", the cause in plain words. */
  message?: string;
  /** Listening only: time since the hold started; shown as a mono timer after the bars. */
  elapsedMs?: number;
  className?: string;
  style?: CSSProperties;
}

const DEFAULT_LEVELS = [0.33, 0.78, 1, 0.5, 0.72, 0.28];
const BAR_COUNT = 6;
const BAR_MAX = 18;
const BAR_MIN = 3;

function defaultMessage(state: HudState, words?: number): string {
  switch (state) {
    case "listening":
      return "Listening";
    case "processing":
      return "Writing…";
    case "inserted":
      return words === undefined ? "Inserted" : `Inserted · ${words} ${words === 1 ? "word" : "words"}`;
    case "copied":
      return "No text field · copied instead";
    case "error":
      return "Couldn't dictate";
  }
}

/**
 * Dictation HUD (40 px, --hud pill). Never takes focus; announces changes politely
 * (errors assertively). States: listening (red dot, bars, optional timer), processing,
 * inserted (✓ + count), copied, error (glyph + cause; red stays reserved for "live").
 */
export function HudPill({ state, levels, words, message, elapsedMs, className, style }: HudPillProps) {
  const text = message ?? defaultMessage(state, words);
  const lv = (levels ?? DEFAULT_LEVELS).slice(-BAR_COUNT);
  // "0:07", like the Dictate how-to strip (a hold rarely passes ten minutes).
  const timer = state === "listening" && elapsedMs !== undefined ? formatDuration(elapsedMs).replace(/^0(\d:)/, "$1") : null;
  return (
    <div
      className={cx(s.hud, s[state], className)}
      style={style}
      role={state === "error" ? "alert" : "status"}
      aria-live={state === "error" ? "assertive" : "polite"}
      aria-atomic="true"
    >
      {state === "listening" ? (
        <>
          <span className="mn-sr-only">{text}</span>
          <span className={s.dot} aria-hidden />
          <span className={s.bars} aria-hidden>
            {lv.map((l, i) => (
              <span
                // biome-ignore lint/suspicious/noArrayIndexKey: bars are positional
                key={i}
                className={s.bar}
                style={{ height: Math.max(BAR_MIN, Math.round(Math.min(1, Math.max(0, l)) * BAR_MAX)) }}
              />
            ))}
          </span>
          {timer ? (
            <span className={s.timer} aria-hidden>
              {timer}
            </span>
          ) : null}
        </>
      ) : null}
      {state === "processing" ? <span className={s.spinner} aria-hidden /> : null}
      {state === "inserted" ? (
        <span className={s.check} aria-hidden>
          <Check size={12} strokeWidth={3.2} />
        </span>
      ) : null}
      {state === "error" ? (
        <span className={s.alert} aria-hidden>
          <CircleAlert size={14} strokeWidth={2.4} />
        </span>
      ) : null}
      {state === "listening" ? null : (
        <span className={state === "error" ? s.message : undefined}>
          {text}
        </span>
      )}
    </div>
  );
}
