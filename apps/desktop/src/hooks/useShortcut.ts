/**
 * useShortcut(keys, handler, options?) — window-level keyboard shortcut.
 *
 *   useShortcut("mod+k", () => openSearch());
 *   useShortcut("space", togglePlay);              // ignored while typing
 *   useShortcut(["mod+1", "mod+2"], (e, combo) => …);
 *
 * Keys are platform-neutral, matching the kit's `formatShortcut`: "mod" is ⌘ on macOS and
 * Ctrl elsewhere; also "shift", "alt", "ctrl". Named keys: space, enter, esc, comma, up…
 *
 * Options:
 * - enabled (default true)
 * - allowInInputs: fire while focus is in a text field. Default: true for combos with
 *   mod/ctrl/alt, false for bare keys (so "space" never steals typing).
 * - preventDefault (default true)
 */
import { useEffect, useRef } from "react";
import { detectPlatform } from "@minutae/ui";

export interface ShortcutOptions {
  enabled?: boolean;
  allowInInputs?: boolean;
  preventDefault?: boolean;
}

const NAMED: Record<string, string> = {
  space: " ", enter: "enter", return: "enter", esc: "escape", escape: "escape", comma: ",", period: ".",
  up: "arrowup", down: "arrowdown", left: "arrowleft", right: "arrowright", slash: "/",
};

interface Combo { key: string; mod: boolean; ctrl: boolean; alt: boolean; shift: boolean; raw: string }

const isMac = detectPlatform() === "mac";

function parse(raw: string): Combo {
  const parts = raw.toLowerCase().split("+").map((p) => p.trim());
  // "mod++" style isn't supported; "mod+," works.
  const c: Combo = { key: "", mod: false, ctrl: false, alt: false, shift: false, raw };
  for (const p of parts) {
    if (p === "mod" || p === "cmd" || p === "meta") c.mod = true;
    else if (p === "ctrl" || p === "control") c.ctrl = true;
    else if (p === "alt" || p === "option" || p === "opt") c.alt = true;
    else if (p === "shift") c.shift = true;
    else c.key = NAMED[p] ?? p;
  }
  return c;
}

export function isTypingTarget(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  if (t.isContentEditable) return true;
  const tag = t.tagName;
  if (tag === "TEXTAREA" || tag === "SELECT") return true;
  if (tag === "INPUT") {
    const type = (t as HTMLInputElement).type;
    return !["checkbox", "radio", "range", "button", "submit"].includes(type);
  }
  return false;
}

export function matchesShortcut(e: KeyboardEvent, combo: Combo | string): boolean {
  const c = typeof combo === "string" ? parse(combo) : combo;
  const wantMeta = c.mod && isMac;
  const wantCtrl = c.ctrl || (c.mod && !isMac);
  if (e.metaKey !== wantMeta || e.ctrlKey !== wantCtrl || e.altKey !== c.alt) return false;
  // Shift is lenient for punctuation keys whose glyph needs it.
  if (c.shift !== e.shiftKey && /^[a-z0-9 ]$|^(enter|escape|arrow.*)$/.test(c.key)) return false;
  const k = e.key.toLowerCase();
  if (k === c.key) return true;
  // Layout-independent fallback for letters/digits (e.g. ⌥ changes e.key on macOS).
  if (/^[a-z]$/.test(c.key) && e.code === `Key${c.key.toUpperCase()}`) return true;
  if (/^[0-9]$/.test(c.key) && e.code === `Digit${c.key}`) return true;
  if (c.key === "," && e.code === "Comma") return true;
  return false;
}

export function useShortcut(
  keys: string | string[],
  handler: (e: KeyboardEvent, combo: string) => void,
  options: ShortcutOptions = {},
): void {
  const { enabled = true, allowInInputs, preventDefault = true } = options;
  const ref = useRef(handler);
  ref.current = handler;
  const list = Array.isArray(keys) ? keys.join("|") : keys;

  useEffect(() => {
    if (!enabled) return;
    const combos = list.split("|").map(parse);
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.isComposing) return;
      for (const c of combos) {
        if (!matchesShortcut(e, c)) continue;
        const chord = c.mod || c.ctrl || c.alt;
        if (isTypingTarget(e.target) && !(allowInInputs ?? chord)) return;
        // Bare keys (space) must not hijack focused buttons/links either.
        if (!chord && e.target instanceof HTMLElement && e.target.closest("button, a, [role=button], [role=tab], [role=slider], [role=checkbox]")) return;
        if (preventDefault) e.preventDefault();
        ref.current(e, c.raw);
        return;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [list, enabled, allowInInputs, preventDefault]);
}
