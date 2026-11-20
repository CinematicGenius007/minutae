import type { ReactNode } from "react";
import { cx } from "../lib/cx";
import s from "./Page.module.css";

export interface StatProps {
  /** "6h 12m", "9". Tabular figures. */
  value: ReactNode;
  /** Mono caps caption ("Recorded this week"). */
  caption: ReactNode;
  align?: "start" | "end";
  className?: string;
}

export function Stat({ value, caption, align = "start", className }: StatProps) {
  return (
    <div className={cx(s.stat, align === "end" && s.statEnd, className)}>
      <div className={s.statValue}>{value}</div>
      <div className={s.statCaption}>{caption}</div>
    </div>
  );
}
