import * as TG from "@radix-ui/react-toggle-group";
import { cx } from "../lib/cx";
import s from "./Segmented.module.css";

export interface SegmentedOption<V extends string = string> {
  value: V;
  label: string;
  disabled?: boolean;
}

export interface SegmentedProps<V extends string = string> {
  /** 2–4 options. Use a Select for more. */
  options: SegmentedOption<V>[];
  value: V;
  onValueChange: (value: V) => void;
  "aria-label": string;
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
}: SegmentedProps<V>) {
  return (
    <TG.Root
      type="single"
      value={value}
      onValueChange={(v) => {
        // A segmented control always has a value; ignore deselection.
        if (v) onValueChange(v as V);
      }}
      aria-label={ariaLabel}
      disabled={disabled}
      className={cx(s.track, fullWidth && s.full, className)}
    >
      {options.map((o) => (
        <TG.Item key={o.value} value={o.value} disabled={o.disabled} className={s.item}>
          {o.label}
        </TG.Item>
      ))}
    </TG.Root>
  );
}
