/**
 * useAppearance() — applies Settings → General → Appearance to this window's ThemeProvider,
 * on load and live on `settings://changed`. Mounted once in App, so the main window and the
 * HUD / call-prompt overlays all follow the same setting.
 */
import { useEffect } from "react";
import { useTheme } from "@minutae/ui";
import { useSettings } from "./useSettings";

export function useAppearance() {
  const { settings } = useSettings();
  const { setPreference } = useTheme();
  const appearance = settings?.appearance;
  useEffect(() => {
    if (appearance) setPreference(appearance);
  }, [appearance, setPreference]);
}
