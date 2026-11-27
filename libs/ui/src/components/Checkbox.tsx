import * as RC from "@radix-ui/react-checkbox";
import { Check, Minus } from "lucide-react";
import { type ComponentProps, type ReactNode, useId } from "react";
import { cx } from "../lib/cx";
import s from "./Choice.module.css";

export interface CheckboxProps extends Omit<ComponentProps<typeof RC.Root>, "children"> {
  /** Optional label to the right (14 px). */
  label?: ReactNode;
  forceFocus?: boolean;
}

/** 18 px checkbox: 1.5 px ink outline; checked is an ink fill with an on-ink check. */
export function Checkbox({ label, className, id, forceFocus, ...rest }: CheckboxProps) {
  const auto = useId();
  const boxId = id ?? auto;
  const box = (
    <RC.Root id={boxId} className={cx(s.checkbox, forceFocus && s.forceFocus, className)} {...rest}>
      <RC.Indicator className={s.indicator}>
        {rest.checked === "indeterminate" ? <Minus size={11} strokeWidth={3.5} aria-hidden /> : <Check size={11} strokeWidth={3.5} aria-hidden />}
      </RC.Indicator>
    </RC.Root>
  );
  if (!label) return box;
  return (
    <label className={s.row} htmlFor={boxId} data-disabled={rest.disabled ? "" : undefined}>
      {box}
      <span>{label}</span>
    </label>
  );
}
