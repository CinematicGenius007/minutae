import type { ReactNode } from "react";
import { cx } from "../lib/cx";
import s from "./Page.module.css";

export interface EmptyStateProps {
  /** "No meetings yet." */
  title: ReactNode;
  /** One thing to learn: "Join a call and Minutae will ask, or press ⌘R." */
  description?: ReactNode;
  /** One action (a Button), optionally followed by a quiet link. */
  action?: ReactNode;
  align?: "start" | "center";
  className?: string;
}

export function EmptyState({ title, description, action, align = "start", className }: EmptyStateProps) {
  return (
    <div className={cx(s.empty, align === "center" && s.emptyCenter, className)}>
      <h2 className={s.emptyTitle}>{title}</h2>
      {description ? <p className={s.emptyBody}>{description}</p> : null}
      {action ? <div className={s.emptyAction}>{action}</div> : null}
    </div>
  );
}
