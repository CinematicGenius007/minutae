import * as RR from "@radix-ui/react-radio-group";
import { type ComponentProps, type ReactNode, useId } from "react";
import { cx } from "../lib/cx";
import s from "./Choice.module.css";

export interface RadioOption {
  value: string;
  label: ReactNode;
  disabled?: boolean;
}

export interface RadioGroupProps extends Omit<ComponentProps<typeof RR.Root>, "children"> {
  options?: RadioOption[];
  children?: ReactNode;
}

/** Radio group. Pass `options`, or compose `RadioItem`s as children. */
export function RadioGroup({ options, children, className, ...rest }: RadioGroupProps) {
  return (
    <RR.Root className={cx(s.radioGroup, className)} {...rest}>
      {options
        ? options.map((o) => <RadioItem key={o.value} value={o.value} label={o.label} disabled={o.disabled} />)
        : children}
    </RR.Root>
  );
}

export interface RadioItemProps extends Omit<ComponentProps<typeof RR.Item>, "children"> {
  label?: ReactNode;
  forceFocus?: boolean;
}

/** 18 px radio. Checked draws a 5.5 px ink ring. */
export function RadioItem({ label, className, id, forceFocus, ...rest }: RadioItemProps) {
  const auto = useId();
  const itemId = id ?? auto;
  const dot = <RR.Item id={itemId} className={cx(s.radio, forceFocus && s.forceFocus, className)} {...rest} />;
  if (!label) return dot;
  return (
    <label className={s.row} htmlFor={itemId} data-disabled={rest.disabled ? "" : undefined}>
      {dot}
      <span>{label}</span>
    </label>
  );
}
