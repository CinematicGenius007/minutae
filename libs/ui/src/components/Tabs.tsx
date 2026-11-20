import * as RT from "@radix-ui/react-tabs";
import type { ComponentProps, ReactNode } from "react";
import { cx } from "../lib/cx";
import { formatShortcut, type Platform } from "../lib/platform";
import s from "./Tabs.module.css";

/** Tabs root (Radix). Tabs switch views, never values. */
export const Tabs = RT.Root;

export function TabsList({ className, ...rest }: ComponentProps<typeof RT.List>) {
  return <RT.List className={cx(s.list, className)} {...rest} />;
}

export interface TabsTriggerProps extends Omit<ComponentProps<typeof RT.Trigger>, "children"> {
  children: ReactNode;
  /** Mono hint after the label, e.g. "mod+3". */
  shortcut?: string;
  platform?: Platform;
}

export function TabsTrigger({ className, children, shortcut, platform, ...rest }: TabsTriggerProps) {
  return (
    <RT.Trigger className={cx(s.trigger, className)} {...rest}>
      <span className={s.text} data-text={typeof children === "string" ? children : undefined}>
        {children}
      </span>
      {shortcut ? (
        <span className={s.shortcut} aria-hidden>
          {formatShortcut(shortcut, platform)}
        </span>
      ) : null}
    </RT.Trigger>
  );
}

export function TabsContent({ className, ...rest }: ComponentProps<typeof RT.Content>) {
  return <RT.Content className={cx(s.content, className)} {...rest} />;
}
