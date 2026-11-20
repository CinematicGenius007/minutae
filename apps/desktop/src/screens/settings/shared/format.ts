import { localPhrase, type Platform } from "@minutae/ui";

/**
 * Converts a Tauri accelerator ("CommandOrControl+Alt+R", "Alt+Space") into the
 * kit's platform-neutral shortcut syntax ("mod+alt+r", "alt+space").
 */
export function toKitShortcut(accelerator: string): string {
  return accelerator
    .split("+")
    .map((k) => k.trim())
    .filter(Boolean)
    .map((k) => {
      const l = k.toLowerCase();
      if (l === "commandorcontrol" || l === "cmdorctrl" || l === "cmdorcontrol") return "mod";
      if (l === "command" || l === "cmd" || l === "super" || l === "meta") return "cmd";
      if (l === "control" || l === "ctrl") return "ctrl";
      if (l === "alt" || l === "option") return "alt";
      return l;
    })
    .join("+");
}

/** "On this Mac" / "On this PC" / "On this computer". */
export function localTitle(platform: Platform): string {
  const p = localPhrase(platform);
  return p.charAt(0).toUpperCase() + p.slice(1);
}

/** "Mac" / "PC" / "computer". */
export function deviceNoun(platform: Platform): string {
  return platform === "mac" ? "Mac" : platform === "windows" ? "PC" : "computer";
}

/** 812_000 → "812k", 1_200_000 → "1.2M". */
export function formatCount(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(n >= 1e7 ? 0 : 1).replace(/\.0$/, "")}M`;
  if (n >= 1e3) return `${Math.round(n / 1e3)}k`;
  return String(n);
}

export function formatUsd(v: number): string {
  return `$${v.toFixed(2)}`;
}

/** Collapses a home-directory prefix to "~". */
export function tildify(path: string): string {
  return path.replace(/^\/(Users|home)\/[^/]+/, "~").replace(/^[A-Z]:\\Users\\[^\\]+/i, "~");
}

/** Builds a Tauri accelerator from a keydown, or null if only modifiers are held. */
export function acceleratorFromEvent(e: KeyboardEvent, platform: Platform): string | null {
  const key = e.key;
  if (["Shift", "Control", "Alt", "Meta", "AltGraph", "CapsLock", "Fn"].includes(key)) return null;
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push(platform === "mac" ? "Cmd" : "Super");
  let name: string;
  if (e.code === "Space") name = "Space";
  else if (e.code.startsWith("Key")) name = e.code.slice(3);
  else if (e.code.startsWith("Digit")) name = e.code.slice(5);
  else if (/^F\d+$/.test(key)) name = key;
  else name = key.length === 1 ? key.toUpperCase() : key;
  return [...mods, name].join("+");
}

const MODIFIERS = new Set(["commandorcontrol", "cmdorctrl", "cmdorcontrol", "command", "cmd", "super", "meta", "control", "ctrl", "alt", "option", "shift"]);

/**
 * Canonical form for comparing accelerators on one platform: modifiers resolved
 * (CommandOrControl → Cmd on macOS, Ctrl elsewhere), sorted, key upper-cased.
 */
export function normalizeAccelerator(accelerator: string, platform: Platform): string {
  const parts = toKitShortcut(accelerator).split("+").filter(Boolean);
  const mods = new Set<string>();
  let key = "";
  for (const p of parts) {
    if (p === "mod") mods.add(platform === "mac" ? "cmd" : "ctrl");
    else if (p === "cmd") mods.add(platform === "mac" ? "cmd" : "super");
    else if (p === "ctrl" || p === "alt" || p === "shift" || p === "super") mods.add(p);
    else key = p;
  }
  return [...[...mods].sort(), key.toUpperCase()].join("+");
}

/**
 * Why a global shortcut can't be used, or null when it can. A global shortcut fires in every
 * app, so it needs a modifier other than Shift (function keys F1–F24 may stand alone) and must
 * not collide with the other Minutae shortcut. The backend re-checks and may still refuse
 * (e.g. the OS or another app already owns the combination).
 */
export function shortcutProblem(accelerator: string, platform: Platform, taken: { accelerator: string; name: string }[] = []): string | null {
  const parts = accelerator.split("+").map((k) => k.trim()).filter(Boolean);
  const key = parts.find((k) => !MODIFIERS.has(k.toLowerCase())) ?? "";
  const mods = parts.filter((k) => MODIFIERS.has(k.toLowerCase()) && k.toLowerCase() !== "shift");
  if (!key) return "Add a key after the modifiers.";
  if (mods.length === 0 && !/^F([1-9]|1\d|2[0-4])$/.test(key)) {
    const options = platform === "mac" ? "⌘, ⌥ or ⌃" : "Ctrl or Alt";
    return `${key} on its own would fire while you type. Hold ${options} with it.`;
  }
  const mine = normalizeAccelerator(accelerator, platform);
  const clash = taken.find((t) => normalizeAccelerator(t.accelerator, platform) === mine);
  if (clash) return `That's already the ${clash.name} shortcut. Pick another.`;
  return null;
}
