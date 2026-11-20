import { cx } from "../lib/cx";
import s from "./Spinner.module.css";

export interface SpinnerProps {
  className?: string;
  /** Accessible label; omit when the surrounding text already says it. */
  label?: string;
}

/** Small ring for unknown-length waits. Use Progress when progress is known. */
export function Spinner({ className, label }: SpinnerProps) {
  return (
    <span
      className={cx(s.spinner, className)}
      role={label ? "status" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    />
  );
}
