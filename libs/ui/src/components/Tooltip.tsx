import * as RT from "@radix-ui/react-tooltip";
import type { ReactNode } from "react";
import { formatShortcut, type Platform } from "../lib/platform";
import s from "./Hud.module.css";

export const TooltipProvider = RT.Provider;

export interface TooltipProps {
  /** The trigger (rendered via asChild: pass one focusable element). */
  children: ReactNode;
  content: ReactNode;
  /** Mono shortcut after the text, e.g. "mod+k". */
  shortcut?: string;
  side?: "top" | "right" | "bottom" | "left";
  platform?: Platform;
}

/** Small --hud tooltip with an optional shortcut. Needs a TooltipProvider above it. */
export function Tooltip({ children, content, shortcut, side = "top", platform }: TooltipProps) {
  return (
    <RT.Root delayDuration={400}>
      <RT.Trigger asChild>{children}</RT.Trigger>
      <RT.Portal>
        <RT.Content side={side} sideOffset={6} className={s.tooltip}>
          {content}
          {shortcut ? <span className={s.tooltipShortcut}>{formatShortcut(shortcut, platform)}</span> : null}
        </RT.Content>
      </RT.Portal>
    </RT.Root>
  );
}
