import * as TG from "@radix-ui/react-toggle-group";
import { useId } from "react";
import { cx } from "../lib/cx";
import s from "./Segmented.module.css";

export interface SegmentedOption<V extends string = string> {
  value: V;
  label: string;
  /** Short trailing tag (e.g. "Planned"). */
  tag?: string;
  disabled?: boolean;
  /** Reason this option is disabled, exposed to screen readers via aria-describedby and title. */
  disabledReason?: string;
}

export interface SegmentedProps<V extends string = string> {
  /** 2–4 options. Use a Select for more. */
  options: SegmentedOption<V>[];
  value: V;
  onValueChange: (value: V) => void;
  "aria-label": string;
  "aria-describedby"?: string;
  fullWidth?: boolean;
  disabled?: boolean;
  className?: string;
}

/** 34 px segmented control: surface track, raised thumb. Switches a value, not a view. */
export function Segmented<V extends string = string>({
  options,
  value,
  onValueChange,
  fullWidth,
  disabled,
  className,
  "aria-label": ariaLabel,
  "aria-describedby": ariaDescribedBy,
}: SegmentedProps<V>) {
  const baseId = useId();
  const reasons = options.filter((o) => o.disabledReason);
  return (
    <>
      <TG.Root
        type="single"
        value={value}
        onValueChange={(v) => {
          // A segmented control always has a value; ignore deselection and disabled options.
          if (!v) return;
          const opt = options.find((o) => o.value === v);
          if (opt && !opt.disabled) onValueChange(v as V);
        }}
        aria-label={ariaLabel}
        aria-describedby={ariaDescribedBy}
        disabled={disabled}
        className={cx(s.track, fullWidth && s.full, className)}
      >
        {options.map((o) => {
          const reasonId = o.disabledReason ? `${baseId}-reason-${o.value}` : undefined;
          return (
            <TG.Item
              key={o.value}
              value={o.value}
              disabled={o.disabled}
              aria-describedby={reasonId}
              title={o.disabled && o.disabledReason ? o.disabledReason : undefined}
              className={s.item}
            >
              <span>{o.label}</span>
              {o.tag ? (
                <>
                  <span className={s.sep} aria-hidden>
                    ·
                  </span>
                  <span className={s.tag}>{o.tag}</span>
                </>
              ) : null}
            </TG.Item>
          );
        })}
      </TG.Root>
      {reasons.map((o) => (
        <span key={o.value} id={`${baseId}-reason-${o.value}`} hidden>
          {o.disabledReason}
        </span>
      ))}
    </>
  );
}
