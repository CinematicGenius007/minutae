import { useEffect } from "react";

/** Overlay windows are transparent: clear the html/body ground for this route only. */
export function useTransparentWindow() {
  useEffect(() => {
    const html = document.documentElement;
    const body = document.body;
    const prev = [html.style.background, body.style.background, body.style.overflow];
    html.style.background = "transparent";
    body.style.background = "transparent";
    body.style.overflow = "hidden";
    return () => {
      html.style.background = prev[0] ?? "";
      body.style.background = prev[1] ?? "";
      body.style.overflow = prev[2] ?? "";
    };
  }, []);
}
