import type { ReactNode } from "react";
import { cx } from "../lib/cx";
import s from "./Page.module.css";

export interface PageHeaderProps {
  /** Mono caps line above the title ("Thursday, 20 November"). */
  overline?: ReactNode;
  /** The one display title on the screen. */
  title: ReactNode;
  /** Right slot, bottom-aligned (Stats, a primary Button). */
  right?: ReactNode;
  className?: string;
}

export function PageHeader({ overline, title, right, className }: PageHeaderProps) {
  return (
    <header className={cx(s.header, className)}>
      <div className={s.headerText}>
        {overline ? <div className={s.overline}>{overline}</div> : null}
        <h1 className={s.title}>{title}</h1>
      </div>
      {right ? <div className={s.right}>{right}</div> : null}
    </header>
  );
}
