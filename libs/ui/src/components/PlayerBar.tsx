import { Pause, Play } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { cx } from "../lib/cx";
import { formatDuration } from "../lib/format";
import s from "./Media.module.css";

export interface PlayerBarProps {
  playing: boolean;
  onTogglePlay: () => void;
  /** Current position, ms. */
  currentMs: number;
  durationMs: number;
  onSeek?: (ms: number) => void;
  /** Playback rate; shown as "1.5×". */
  speed?: number;
  /** Rates the speed button cycles through. */
  speeds?: number[];
  onSpeedChange?: (speed: number) => void;
  /** Extra controls on the right (e.g. a quiet "Show in Finder"). */
  extra?: ReactNode;
  disabled?: boolean;
  className?: string;
}

function speedLabel(v: number): string {
  return `${Number.isInteger(v) ? v.toFixed(0) : String(v)}×`;
}

/** 76 px bottom bar: play/pause, time, scrubber, speed. */
export function PlayerBar({
  playing,
  onTogglePlay,
  currentMs,
  durationMs,
  onSeek,
  speed = 1,
  speeds = [1, 1.25, 1.5, 2],
  onSpeedChange,
  extra,
  disabled,
  className,
}: PlayerBarProps) {
  const pct = durationMs > 0 ? Math.min(100, Math.max(0, (currentMs / durationMs) * 100)) : 0;
  const nextSpeed = () => {
    const i = speeds.indexOf(speed);
    onSpeedChange?.(speeds[(i + 1) % speeds.length] ?? 1);
  };
  return (
    <div className={cx(s.player, className)} role="group" aria-label="Playback">
      <button
        type="button"
        className={s.play}
        onClick={onTogglePlay}
        disabled={disabled}
        aria-label={playing ? "Pause" : "Play"}
      >
        {playing ? <Pause size={16} strokeWidth={2} fill="currentColor" /> : <Play size={16} strokeWidth={2} fill="currentColor" />}
      </button>
      <span className={s.time}>
        {formatDuration(currentMs)} <span className={s.timeTotal}>/ {formatDuration(durationMs)}</span>
      </span>
      <input
        type="range"
        className={s.scrub}
        min={0}
        max={Math.max(0, durationMs)}
        step={1000}
        value={Math.min(currentMs, durationMs)}
        onChange={(e) => onSeek?.(Number(e.currentTarget.value))}
        disabled={disabled}
        aria-label="Seek"
        aria-valuetext={`${formatDuration(currentMs)} of ${formatDuration(durationMs)}`}
        style={{ "--pct": `${pct}%` } as CSSProperties}
      />
      <button
        type="button"
        className={s.speed}
        onClick={nextSpeed}
        disabled={disabled}
        aria-label={`Playback speed ${speedLabel(speed)}`}
      >
        {speedLabel(speed)}
      </button>
      {extra ? <div className={s.extra}>{extra}</div> : null}
    </div>
  );
}
