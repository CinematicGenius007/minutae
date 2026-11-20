import type { CSSProperties } from "react";
import { cx } from "../lib/cx";
import s from "./Media.module.css";

export interface WaveformProps {
  /** Levels 0–1, oldest first. */
  levels: number[];
  /** Bar height at level 1, px. Default 32. */
  height?: number;
  barWidth?: number;
  gap?: number;
  /** Last N bars draw in --onair (live recording tail). Only while recording. */
  liveTail?: number;
  /** 0–1: bars before this fraction draw as played (ink), after as faint. Omit for all ink. */
  progress?: number;
  /** Animate height changes (live input). */
  animated?: boolean;
  label?: string;
  className?: string;
}

/** Bar waveform from an array of levels. */
export function Waveform({
  levels,
  height = 32,
  barWidth = 2,
  gap = 2,
  liveTail = 0,
  progress,
  animated,
  label = "Audio waveform",
  className,
}: WaveformProps) {
  const n = levels.length;
  const playedUntil = progress === undefined ? n : Math.round(Math.min(1, Math.max(0, progress)) * n);
  const style: CSSProperties = { height, gap };
  return (
    <div className={cx(s.wave, animated && s.animated, className)} style={style} role="img" aria-label={label}>
      {levels.map((l, i) => {
        const h = Math.max(barWidth, Math.round(Math.min(1, Math.max(0, l)) * height));
        const live = liveTail > 0 && i >= n - liveTail;
        return (
          <span
            // biome-ignore lint/suspicious/noArrayIndexKey: bars are positional
            key={i}
            className={cx(s.bar, live ? s.live : i >= playedUntil && s.past)}
            style={{ width: barWidth, height: h }}
          />
        );
      })}
    </div>
  );
}
