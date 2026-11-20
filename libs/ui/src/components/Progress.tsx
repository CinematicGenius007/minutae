import type { CSSProperties, ReactNode } from "react";
import { clamp01 } from "../lib/format";
import { cx } from "../lib/cx";
import s from "./Status.module.css";

export interface ProgressProps {
  /** 0–1. */
  value: number;
  /** Mono caption under the track, e.g. "Transcribing · 62% · ~2 min left". */
  caption?: ReactNode;
  /** Accessible name; defaults to the caption when it is a string. */
  label?: string;
  /** Track width in px or any CSS length; default fills the container. */
  width?: number | string;
  /** Right-align the caption (status lane). */
  align?: "start" | "end";
  className?: string;
}

/** 3 px progress bar. Never use a spinner when progress is known. */
export function Progress({ value, caption, label, width, align = "start", className }: ProgressProps) {
  const v = clamp01(value);
  const style: CSSProperties | undefined = width !== undefined ? { width } : undefined;
  return (
    <div className={cx(s.progress, align === "end" && s.end, className)} style={style}>
      <div
        className={s.track}
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(v * 100)}
        aria-label={label ?? (typeof caption === "string" ? caption : undefined)}
        style={width !== undefined ? { width: "100%" } : undefined}
      >
        <div className={s.fill} style={{ width: `${v * 100}%` }} />
      </div>
      {caption ? <div className={s.caption}>{caption}</div> : null}
    </div>
  );
}
