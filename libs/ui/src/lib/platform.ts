export type Platform = "mac" | "windows" | "linux";

/** Detects the OS from a user-agent string (Tauri webviews report the host OS). */
export function detectPlatform(userAgent?: string): Platform {
  const ua = userAgent ?? (typeof navigator !== "undefined" ? navigator.userAgent : "");
  if (/Mac|iPhone|iPad|iPod/i.test(ua)) return "mac";
  if (/Win/i.test(ua)) return "windows";
  return "linux";
}

/**
 * Canonical key names accepted by `shortcutKeys`. Write shortcuts with
 * mac-style names ("mod" = ⌘ on macOS, Ctrl elsewhere).
 */
export type ModifierKey = "mod" | "cmd" | "ctrl" | "alt" | "option" | "shift";

const MAC: Record<string, string> = {
  mod: "⌘", cmd: "⌘", meta: "⌘", ctrl: "⌃", control: "⌃",
  alt: "⌥", option: "⌥", opt: "⌥", shift: "⇧",
};
const OTHER: Record<string, string> = {
  mod: "Ctrl", cmd: "Ctrl", meta: "Ctrl", ctrl: "Ctrl", control: "Ctrl",
  alt: "Alt", option: "Alt", opt: "Alt", shift: "Shift",
};
const NAMED: Record<string, string> = {
  enter: "⏎", return: "⏎", esc: "esc", escape: "esc", space: "Space",
  up: "↑", down: "↓", left: "←", right: "→", tab: "Tab",
  backspace: "⌫", delete: "⌦", comma: ",", period: ".",
};

/**
 * Splits a shortcut like "mod+k" or "alt+space" into display glyphs for the
 * platform: ["⌘","K"] on macOS, ["Ctrl","K"] on Windows/Linux.
 */
export function shortcutKeys(shortcut: string, platform: Platform = detectPlatform()): string[] {
  const table = platform === "mac" ? MAC : OTHER;
  return shortcut
    .split("+")
    .map((raw) => raw.trim())
    .filter((raw) => raw.length > 0)
    .map((raw) => {
      const key = raw.toLowerCase();
      if (key in table) return table[key] as string;
      if (key in NAMED) return NAMED[key] as string;
      return raw.length === 1 ? raw.toUpperCase() : raw[0]?.toUpperCase() + raw.slice(1);
    });
}

/**
 * Formats a shortcut as one string: "⌘K" on macOS (glyphs run together),
 * "Ctrl+K" elsewhere (words joined by +).
 */
export function formatShortcut(shortcut: string, platform: Platform = detectPlatform()): string {
  const keys = shortcutKeys(shortcut, platform);
  return platform === "mac" ? keys.join("") : keys.join("+");
}

/** "on this Mac" / "on this PC" / "on this computer" (§10). */
export function localPhrase(platform: Platform = detectPlatform()): string {
  return platform === "mac" ? "on this Mac" : platform === "windows" ? "on this PC" : "on this computer";
}
