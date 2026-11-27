import * as RS from "@radix-ui/react-select";
import { Check, ChevronDown } from "lucide-react";
import { cx } from "../lib/cx";
import m from "./Menu.module.css";

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
  /** Muted row, e.g. "Follow system default". */
  muted?: boolean;
}

/** An option, or "separator" for a hairline between groups. */
export type SelectEntry = SelectOption | "separator";

export interface SelectProps {
  options: SelectEntry[];
  value?: string;
  defaultValue?: string;
  onValueChange?: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  /** Accessible name when there is no visible label. */
  "aria-label"?: string;
  "aria-labelledby"?: string;
  "aria-describedby"?: string;
  id?: string;
  name?: string;
  /** Stretch the trigger to its container. */
  fullWidth?: boolean;
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  className?: string;
}

/**
 * 32 px select. Menu is raised, radius 10, rows 32, with a 14 px checkmark lane.
 * Keyboard: ↑↓ ⏎ esc and type-ahead (Radix).
 */
export function Select({
  options,
  placeholder,
  fullWidth,
  className,
  disabled,
  id,
  "aria-label": ariaLabel,
  "aria-labelledby": ariaLabelledBy,
  "aria-describedby": ariaDescribedBy,
  ...root
}: SelectProps) {
  return (
    <RS.Root disabled={disabled} {...root}>
      <RS.Trigger
        id={id}
        aria-label={ariaLabel}
        aria-labelledby={ariaLabelledBy}
        aria-describedby={ariaDescribedBy}
        className={cx(m.trigger, fullWidth && m.full, className)}
      >
        <span className={m.value}>
          <RS.Value placeholder={placeholder} />
        </span>
        <RS.Icon className={m.chevron}>
          <ChevronDown size={10} strokeWidth={2.6} aria-hidden />
        </RS.Icon>
      </RS.Trigger>
      <RS.Portal>
        <RS.Content className={m.content} position="popper" sideOffset={6} align="start" collisionPadding={8}>
          <RS.Viewport className={m.viewport}>
            {options.map((o, i) =>
              o === "separator" ? (
                // biome-ignore lint/suspicious/noArrayIndexKey: separators have no identity
                <RS.Separator key={`sep-${i}`} className={m.separator} />
              ) : (
                <RS.Item key={o.value} value={o.value} disabled={o.disabled} className={cx(m.item, o.muted && m.muted)}>
                  <span className={m.check} aria-hidden>
                    <RS.ItemIndicator>
                      <Check size={12} strokeWidth={3} aria-hidden />
                    </RS.ItemIndicator>
                  </span>
                  <RS.ItemText>{o.label}</RS.ItemText>
                </RS.Item>
              ),
            )}
          </RS.Viewport>
        </RS.Content>
      </RS.Portal>
    </RS.Root>
  );
}

export interface MenuPreviewProps {
  options: SelectEntry[];
  value?: string;
  /** Row value drawn in its highlighted state. */
  highlighted?: string;
  className?: string;
}

/** Static, non-interactive picture of an open Select menu (gallery, screenshots). */
export function MenuPreview({ options, value, highlighted, className }: MenuPreviewProps) {
  return (
    <div className={cx(m.content, className)} aria-hidden style={{ position: "relative" }}>
      <div className={m.viewport}>
        {options.map((o, i) =>
          o === "separator" ? (
            // biome-ignore lint/suspicious/noArrayIndexKey: separators have no identity
            <div key={`sep-${i}`} className={m.separator} />
          ) : (
            <div
              key={o.value}
              className={cx(m.item, o.muted && m.muted)}
              data-highlighted={o.value === highlighted ? "" : undefined}
            >
              <span className={m.check}>{o.value === value ? <Check size={12} strokeWidth={3} /> : null}</span>
              <span className={m.itemText}>{o.label}</span>
            </div>
          ),
        )}
      </div>
    </div>
  );
}
