import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useState } from "react";

export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

export interface ThemeContextValue {
  /** What the user picked. */
  preference: ThemePreference;
  /** What is on screen right now. */
  resolved: ResolvedTheme;
  setPreference: (next: ThemePreference) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

const QUERY = "(prefers-color-scheme: dark)";

function systemTheme(): ResolvedTheme {
  if (typeof window === "undefined" || !window.matchMedia) return "light";
  return window.matchMedia(QUERY).matches ? "dark" : "light";
}

function readStored(key: string | null): ThemePreference | null {
  if (!key) return null;
  try {
    const v = window.localStorage.getItem(key);
    return v === "light" || v === "dark" || v === "system" ? v : null;
  } catch {
    return null;
  }
}

function writeStored(key: string | null, value: ThemePreference): void {
  if (!key) return;
  try {
    window.localStorage.setItem(key, value);
  } catch {
    /* storage unavailable: preference lives for this session only */
  }
}

/**
 * Applies a theme preference to `<html data-theme>`.
 * 'system' removes the attribute so tokens.css follows `prefers-color-scheme`.
 */
export function applyThemePreference(preference: ThemePreference, root?: HTMLElement): void {
  const el = root ?? (typeof document !== "undefined" ? document.documentElement : undefined);
  if (!el) return;
  if (preference === "system") el.removeAttribute("data-theme");
  else el.setAttribute("data-theme", preference);
}

export interface ThemeProviderProps {
  children: ReactNode;
  /** Initial preference when nothing is stored. Default 'system'. */
  defaultPreference?: ThemePreference;
  /** Controlled preference (e.g. from app settings). */
  preference?: ThemePreference;
  onPreferenceChange?: (next: ThemePreference) => void;
  /** localStorage key; pass null to disable persistence. */
  storageKey?: string | null;
}

export function ThemeProvider({
  children,
  defaultPreference = "system",
  preference: controlled,
  onPreferenceChange,
  storageKey = "minutae.theme",
}: ThemeProviderProps) {
  const [uncontrolled, setUncontrolled] = useState<ThemePreference>(
    () => readStored(storageKey) ?? defaultPreference,
  );
  const preference = controlled ?? uncontrolled;
  const [system, setSystem] = useState<ResolvedTheme>(systemTheme);

  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) return;
    const mql = window.matchMedia(QUERY);
    const onChange = () => setSystem(mql.matches ? "dark" : "light");
    mql.addEventListener("change", onChange);
    return () => mql.removeEventListener("change", onChange);
  }, []);

  useEffect(() => {
    applyThemePreference(preference);
  }, [preference]);

  const setPreference = useCallback(
    (next: ThemePreference) => {
      if (controlled === undefined) setUncontrolled(next);
      writeStored(storageKey, next);
      onPreferenceChange?.(next);
    },
    [controlled, onPreferenceChange, storageKey],
  );

  const value = useMemo<ThemeContextValue>(
    () => ({ preference, resolved: preference === "system" ? system : preference, setPreference }),
    [preference, system, setPreference],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

/** Reads the theme. Throws outside a ThemeProvider. */
export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used inside <ThemeProvider>");
  return ctx;
}

/** Like useTheme, but returns null outside a provider. */
export function useOptionalTheme(): ThemeContextValue | null {
  return useContext(ThemeContext);
}
