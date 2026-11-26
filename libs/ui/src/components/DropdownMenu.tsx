import * as DM from "@radix-ui/react-dropdown-menu";
import { Check } from "lucide-react";
import type { ReactNode } from "react";
import { cx } from "../lib/cx";
import { formatShortcut, type Platform } from "../lib/platform";
import m from "./Menu.module.css";

export interface MenuAction {
  label: string;
  onSelect?: () => void;
  shortcut?: string;
  checked?: boolean;
  disabled?: boolean;
  /** Destructive text (never a red fill). */
  destructive?: boolean;
}

export type MenuEntry = MenuAction | "separator";

export interface DropdownMenuProps {
  /** The element that opens the menu (rendered via asChild, so pass a focusable element). */
  trigger: ReactNode;
  items: MenuEntry[];
  align?: "start" | "center" | "end";
  platform?: Platform;
}

/** Action menu with the same look as the Select menu. */
export function DropdownMenu({ trigger, items, align = "start", platform }: DropdownMenuProps) {
  const hasChecks = items.some((i) => i !== "separator" && i.checked !== undefined);
  return (
    <DM.Root>
      <DM.Trigger asChild>{trigger}</DM.Trigger>
      <DM.Portal>
        <DM.Content className={m.content} sideOffset={6} align={align} collisionPadding={8}>
          {items.map((it, i) =>
            it === "separator" ? (
              // biome-ignore lint/suspicious/noArrayIndexKey: separators have no identity
              <DM.Separator key={`sep-${i}`} className={m.separator} />
            ) : it.checked !== undefined ? (
              <DM.CheckboxItem
                key={it.label}
                className={cx(m.item, it.destructive && m.danger)}
                checked={it.checked}
                disabled={it.disabled}
                onCheckedChange={() => it.onSelect?.()}
              >
                <span className={m.check} aria-hidden>
                  <DM.ItemIndicator>
                    <Check size={12} strokeWidth={3} aria-hidden />
                  </DM.ItemIndicator>
                </span>
                <span className={m.itemText}>{it.label}</span>
                {it.shortcut ? (
                  <span className={m.itemShortcut} aria-hidden>
                    {formatShortcut(it.shortcut, platform)}
                  </span>
                ) : null}
              </DM.CheckboxItem>
            ) : (
              <DM.Item
                key={it.label}
                className={cx(m.item, it.destructive && m.danger)}
                disabled={it.disabled}
                onSelect={it.onSelect}
              >
                {hasChecks ? <span className={m.check} aria-hidden /> : null}
                <span className={m.itemText}>{it.label}</span>
                {it.shortcut ? (
                  <span className={m.itemShortcut} aria-hidden>
                    {formatShortcut(it.shortcut, platform)}
                  </span>
                ) : null}
              </DM.Item>
            ),
          )}
        </DM.Content>
      </DM.Portal>
    </DM.Root>
  );
}
