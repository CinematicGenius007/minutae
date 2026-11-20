import type { CSSProperties, MouseEvent, ReactNode } from "react";
import { cx } from "../lib/cx";
import s from "./List.module.css";

/** Lane widths in px. `false` removes a lane. Defaults: time 64, people 180, duration 72, status 132. */
export interface LaneWidths {
  time?: number | false;
  people?: number | false;
  duration?: number | false;
  status?: number | false;
}

export interface LaneRowProps {
  time?: ReactNode;
  title: ReactNode;
  subtitle?: ReactNode;
  people?: ReactNode;
  duration?: ReactNode;
  /** Status lane content: a Pill, a Progress, a StatusDot … */
  status?: ReactNode;
  lanes?: LaneWidths;
  /**
   * Makes the row clickable. The title becomes the row's button (keyboard focus, Enter/Space),
   * so other controls in the row (a "Retry" in the status lane) are not nested inside it.
   */
  onClick?: () => void;
  selected?: boolean;
  className?: string;
  "aria-label"?: string;
}

/**
 * List row with fixed lanes so values line up across rows; only the body flexes.
 */
export function LaneRow({
  time,
  title,
  subtitle,
  people,
  duration,
  status,
  lanes = {},
  onClick,
  selected,
  className,
  "aria-label": ariaLabel,
}: LaneRowProps) {
  const style: Record<string, string> = {};
  for (const key of ["time", "people", "duration", "status"] as const) {
    const w = lanes[key];
    if (typeof w === "number") style[`--lane-${key}`] = `${w}px`;
  }
  const show = (key: keyof LaneWidths) => lanes[key] !== false;
  const activate = onClick
    ? (e: MouseEvent<HTMLButtonElement>) => {
        e.stopPropagation();
        onClick();
      }
    : undefined;
  return (
    // biome-ignore lint/a11y/useKeyWithClickEvents lint/a11y/noStaticElementInteractions: mouse convenience; the title button is the keyboard path
    <div
      className={cx(s.lane, onClick && s.interactive, selected && s.selected, className)}
      style={style as CSSProperties}
      onClick={onClick}
    >
      {show("time") ? <div className={s.laneTime}>{time}</div> : null}
      <div className={s.laneBody}>
        {onClick ? (
          <button
            type="button"
            className={cx(s.laneTitle, s.laneButton)}
            aria-label={ariaLabel}
            aria-current={selected || undefined}
            onClick={activate}
          >
            {title}
          </button>
        ) : (
          <div className={s.laneTitle}>
            {title}
          </div>
        )}
        {subtitle ? <div className={s.laneSubtitle}>{subtitle}</div> : null}
      </div>
      {show("people") ? <div className={s.lanePeople}>{people}</div> : null}
      {show("duration") ? <div className={s.laneDuration}>{duration}</div> : null}
      {show("status") ? <div className={s.laneStatus}>{status}</div> : null}
    </div>
  );
}
