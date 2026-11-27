import type { HTMLAttributes, ReactNode, Ref } from "react";
import { cx } from "../lib/cx";
import s from "./List.module.css";

export interface GroupLabelProps extends Omit<HTMLAttributes<HTMLElement>, "children"> {
  children: ReactNode;
  /** Mono count on the right. */
  count?: ReactNode;
  as?: "h2" | "h3" | "h4" | "div";
  ref?: Ref<HTMLElement>;
}

/** Mono caps label above a list, 10 px gap, 1 px ink rule. */
export function GroupLabel({ children, count, as: Tag = "h3", className, ref, ...rest }: GroupLabelProps) {
  return (
    <Tag ref={ref as Ref<HTMLHeadingElement & HTMLDivElement>} className={cx(s.group, className)} {...rest}>
      <span className={s.groupLabel}>{children}</span>
      {count !== undefined ? <span className={s.groupCount}>{count}</span> : null}
    </Tag>
  );
}
