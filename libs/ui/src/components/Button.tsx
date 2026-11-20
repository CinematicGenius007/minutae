import type { ButtonHTMLAttributes, ReactNode, Ref } from "react";
import { cx } from "../lib/cx";
import { formatShortcut, type Platform } from "../lib/platform";
import s from "./Button.module.css";
import { Spinner } from "./Spinner";

export type ButtonVariant = "primary" | "secondary" | "quiet" | "destructive";
export type ButtonSize = "md" | "hero";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** primary = ink fill (one per view); secondary = hairline; quiet / destructive = underlined text. */
  variant?: ButtonVariant;
  /** md = 36 px, hero = 52 px. */
  size?: ButtonSize;
  /** Shows a spinner, sets aria-busy and blocks clicks. */
  loading?: boolean;
  /** Leading 16 px icon (lucide). */
  icon?: ReactNode;
  /** Trailing shortcut, e.g. "mod+enter". */
  shortcut?: string;
  platform?: Platform;
  ref?: Ref<HTMLButtonElement>;
}

export function Button({
  variant = "primary",
  size = "md",
  loading = false,
  icon,
  shortcut,
  platform,
  className,
  children,
  disabled,
  type = "button",
  ref,
  ...rest
}: ButtonProps) {
  return (
    <button
      ref={ref}
      type={type}
      className={cx(s.button, s[variant], size === "hero" && s.hero, loading && s.loading, className)}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      {...rest}
    >
      {loading ? <Spinner /> : icon ? <span className={s.icon} aria-hidden>{icon}</span> : null}
      <span className={s.label}>{children}</span>
      {shortcut ? <span className={s.shortcut} aria-hidden>{formatShortcut(shortcut, platform)}</span> : null}
    </button>
  );
}
