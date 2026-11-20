import type { HTMLAttributes, ReactNode } from "react";
import { cx } from "../lib/cx";
import s from "./Status.module.css";

export type StatusTone = "ok" | "onair" | "off";

export interface StatusDotProps extends HTMLAttributes<HTMLSpanElement> {
  tone: StatusTone;
  /** Always pair the dot with a word (§2): Ready, On air, Off. */
  label: ReactNode;
  /** body = 13 px text; caps = mono 11 uppercase (sidebar mic status). */
  variant?: "body" | "caps";
  /** Pulse the on-air dot (default true for tone onair). */
  pulse?: boolean;
}

/** 6 px dot plus a word. Colour never carries meaning alone. */
export function StatusDot({ tone, label, variant = "body", pulse, className, ...rest }: StatusDotProps) {
  const pulsing = pulse ?? tone === "onair";
  return (
    <span className={cx(s.status, s[variant], tone === "off" && s.isOff, className)} {...rest}>
      <span className={cx(s.dot, s[tone], pulsing && s.pulse)} aria-hidden />
      <span className={s.label}>{label}</span>
    </span>
  );
}
