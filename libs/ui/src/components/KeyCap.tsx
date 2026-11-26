import type { HTMLAttributes } from "react";
import { cx } from "../lib/cx";
import { formatShortcut, type Platform, shortcutKeys } from "../lib/platform";
import { usePlatform } from "../theme/usePlatform";
import s from "./KeyCap.module.css";

export interface KeyCapProps extends HTMLAttributes<HTMLElement> {
  size?: "md" | "lg";
}

/** One physical key. 24 px (md) or 32 px (lg). */
export function KeyCap({ size = "md", className, ...rest }: KeyCapProps) {
  return <kbd className={cx(s.cap, size === "lg" && s.lg, className)} {...rest} />;
}

const SPOKEN: Record<string, string> = {
  "⌘": "Command",
  "⌥": "Option",
  "⌃": "Control",
  "⇧": "Shift",
  "⏎": "Return",
  "↑": "Up",
  "↓": "Down",
  "←": "Left",
  "→": "Right",
  "⌫": "Backspace",
  "⌦": "Delete",
  esc: "Escape",
};

export interface ShortcutProps {
  /** Platform-neutral shortcut: "mod+k", "alt+space", "mod+,". */
  keys: string;
  /** caps = keycaps; text = inline mono glyphs (nav hints, field hints). */
  variant?: "caps" | "text";
  size?: "sm" | "md" | "lg";
  /** Override detection (tests, gallery). */
  platform?: Platform;
  className?: string;
}

/** Renders ⌘/⌥ on macOS and Ctrl/Alt on Windows and Linux. */
export function Shortcut({ keys, variant = "text", size = "md", platform: override, className }: ShortcutProps) {
  const platform = usePlatform(override);
  const parts = shortcutKeys(keys, platform);
  const spoken = parts.map((k) => SPOKEN[k] ?? k).join(" ");
  if (variant === "text") {
    return (
      <kbd className={cx(s.text, size === "sm" && s.textSm, className)}>
        <span aria-hidden>{formatShortcut(keys, platform)}</span>
        <span className="mn-sr-only">{spoken}</span>
      </kbd>
    );
  }
  return (
    <kbd className={cx(s.combo, className)}>
      <span className="mn-sr-only">{spoken}</span>
      {parts.map((k, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: keys can repeat and never reorder
        <KeyCap key={i} size={size === "lg" ? "lg" : "md"} aria-hidden>
          {k}
        </KeyCap>
      ))}
    </kbd>
  );
}
