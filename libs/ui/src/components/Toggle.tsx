import * as RSw from "@radix-ui/react-switch";
import type { ComponentProps } from "react";
import { cx } from "../lib/cx";
import s from "./Choice.module.css";

export interface ToggleProps extends Omit<ComponentProps<typeof RSw.Root>, "children"> {
  /** Forces the focus ring (gallery). */
  forceFocus?: boolean;
}

/** 24 × 40 switch. Applies instantly; there are no Save buttons. */
export function Toggle({ className, forceFocus, ...rest }: ToggleProps) {
  return (
    <RSw.Root className={cx(s.switch, forceFocus && s.forceFocus, className)} {...rest}>
      <RSw.Thumb className={s.thumb} />
    </RSw.Root>
  );
}
