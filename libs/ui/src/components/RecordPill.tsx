import { type ButtonHTMLAttributes, type Ref, useEffect, useRef, useState } from "react";
import { cx } from "../lib/cx";
import { formatDuration } from "../lib/format";
import { formatShortcut, type Platform } from "../lib/platform";
import s from "./RecordPill.module.css";

export interface RecordPillProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  /** Live = recording: on-air fill, stop square, timer. */
  live?: boolean;
  /** Elapsed time while live, in ms. */
  elapsedMs?: number;
  /** Shortcut shown when idle. Default "mod+r". */
  shortcut?: string;
  idleLabel?: string;
  liveLabel?: string;
  /** Extra words for screen readers only, appended to the visible label ("open the live view"). */
  srHint?: string;
  /** Show the mono timer while live (default true). Off when a larger timer sits nearby. */
  showTimer?: boolean;
  /** Stretch to the container (sidebar). */
  fullWidth?: boolean;
  platform?: Platform;
  ref?: Ref<HTMLButtonElement>;
}

/**
 * The record button. Idle: ink pill with an on-air dot. Live: on-air pill with
 * a stop square and a mono timer. The timer is announced once a minute, not
 * every second (§11).
 */
export function RecordPill({
  live = false,
  elapsedMs = 0,
  shortcut = "mod+r",
  idleLabel = "Record",
  liveLabel = "Recording",
  fullWidth = false,
  showTimer = true,
  srHint,
  platform,
  className,
  ref,
  ...rest
}: RecordPillProps) {
  const minutes = Math.floor(elapsedMs / 60_000);
  const wasLive = useRef(live);
  const [announcement, setAnnouncement] = useState("");
  useEffect(() => {
    if (live) {
      if (!wasLive.current || minutes === 0) setAnnouncement("Recording started");
      else setAnnouncement(`Recording, ${minutes} ${minutes === 1 ? "minute" : "minutes"}`);
    } else if (wasLive.current) {
      setAnnouncement("Recording stopped");
    }
    wasLive.current = live;
  }, [live, minutes]);
  // The accessible name is the visible label (WCAG 2.5.3), plus an optional hidden hint.
  // The minute announcement sits outside the button so it doesn't change the name.
  return (
    <>
      <button
        ref={ref}
        type="button"
        className={cx(s.pill, live && s.live, fullWidth && s.full, className)}
        aria-pressed={live}
        {...rest}
      >
        <span className={s.lead}>
          <span className={live ? s.stop : s.dot} aria-hidden />
          <span className={s.label}>{live ? liveLabel : idleLabel}</span>
        </span>
        {srHint ? <span className="mn-sr-only">, {srHint}</span> : null}
        {live ? (
          showTimer ? <span className={cx(s.trail, s.timer)} aria-hidden>{formatDuration(elapsedMs)}</span> : null
        ) : shortcut ? (
          <span className={cx(s.trail, s.shortcut)} aria-hidden>{formatShortcut(shortcut, platform)}</span>
        ) : null}
      </button>
      <span className="mn-sr-only" aria-live="polite">{announcement}</span>
    </>
  );
}
