// Route table (hash routing, so it works from a file:// or tauri:// origin).
// Windows: "main" loads "/#/", "hud" loads "/#/hud", "prompt" loads "/#/prompt".
import { lazy } from "react";

export const screens = {
  meetings: lazy(() => import("./screens/meetings")),
  live: lazy(() => import("./screens/live")),
  detail: lazy(() => import("./screens/detail")),
  dictate: lazy(() => import("./screens/dictate")),
  library: lazy(() => import("./screens/library")),
  settings: lazy(() => import("./screens/settings")),
  onboarding: lazy(() => import("./screens/onboarding")),
  hud: lazy(() => import("./screens/overlays/Hud")),
  prompt: lazy(() => import("./screens/overlays/CallPrompt")),
};

/** Path patterns (wouter syntax). */
export const paths = {
  meetings: "/",
  detail: "/meetings/:id",
  live: "/live",
  dictate: "/dictate",
  library: "/library",
  settings: "/settings/:section?",
  onboarding: "/onboarding",
  gallery: "/gallery",
  hud: "/hud",
  prompt: "/prompt",
} as const;
