import { X } from "lucide-react";
import type { ButtonHTMLAttributes, HTMLAttributes, ReactNode } from "react";
import { cx } from "../lib/cx";
import s from "./Chip.module.css";

export interface FilterChipProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  selected?: boolean;
  /** Optional mono count after the label. */
  count?: number;
}

/** 30 px filter chip. Selected: ink fill. Unselected: hairline. */
export function FilterChip({ selected = false, count, className, children, type = "button", ...rest }: FilterChipProps) {
  return (
    <button type={type} aria-pressed={selected} className={cx(s.chip, className)} {...rest}>
      {children}
      {count !== undefined ? <span className={s.count}>{count}</span> : null}
    </button>
  );
}

export interface TagProps extends HTMLAttributes<HTMLSpanElement> {
  children: ReactNode;
  /** Shows a × that calls this. */
  onRemove?: () => void;
  /** Accessible name for the × (defaults to "Remove <children>"). */
  removeLabel?: string;
}

/** Tag: surface fill, optional ×. */
export function Tag({ children, onRemove, removeLabel, className, ...rest }: TagProps) {
  return (
    <span className={cx(s.tag, !onRemove && s.tagStatic, className)} {...rest}>
      {children}
      {onRemove ? (
        <button
          type="button"
          className={s.remove}
          onClick={onRemove}
          aria-label={removeLabel ?? `Remove ${typeof children === "string" ? children : "tag"}`}
        >
          <X size={12} strokeWidth={2.4} />
        </button>
      ) : null}
    </span>
  );
}

/** Small outline label for status lanes ("3 actions", "Done"). Not interactive. */
export function Pill({ className, ...rest }: HTMLAttributes<HTMLSpanElement>) {
  return <span className={cx(s.pill, className)} {...rest} />;
}
