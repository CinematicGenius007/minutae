import type { AnchorHTMLAttributes, ButtonHTMLAttributes, ReactNode } from "react";
import { cx } from "../lib/cx";
import { formatShortcut, type Platform } from "../lib/platform";
import s from "./Sidebar.module.css";
import { StatusDot, type StatusTone } from "./StatusDot";

export interface SidebarProps {
  /** Nav items (NavItem). */
  children: ReactNode;
  /** Bottom area: MicStatus and RecordPill. */
  footer?: ReactNode;
  /** Replaces the default "minutae ●" wordmark. */
  wordmark?: ReactNode;
  /** Extra top padding for macOS traffic lights (px). */
  topInset?: number;
  className?: string;
  "aria-label"?: string;
}

/** 240 px sidebar: wordmark, four places, and at the bottom mic status and the Record pill. */
export function Sidebar({ children, footer, wordmark, topInset, className, "aria-label": ariaLabel = "Main" }: SidebarProps) {
  return (
    <aside
      className={cx(s.sidebar, className)}
      style={topInset ? { paddingTop: `calc(var(--s-8) + ${topInset}px)` } : undefined}
    >
      <div className={s.top}>
        {wordmark ?? <Wordmark />}
        <nav className={s.nav} aria-label={ariaLabel}>
          {children}
        </nav>
      </div>
      {footer ? <div className={s.bottom}>{footer}</div> : null}
    </aside>
  );
}

/** "minutae ●" — 22/800 Geist with a 7 px ink dot. */
export function Wordmark({ className }: { className?: string }) {
  return (
    <div className={cx(s.wordmark, className)}>
      <span className={s.wordmarkText}>minutae</span>
      <span className={s.wordmarkDot} aria-hidden />
    </div>
  );
}

interface NavItemBase {
  active?: boolean;
  /** Mono hint on the right, e.g. "mod+1". */
  shortcut?: string;
  platform?: Platform;
  children: string;
}

export type NavItemProps =
  | (NavItemBase & { href: string } & Omit<AnchorHTMLAttributes<HTMLAnchorElement>, "children">)
  | (NavItemBase & { href?: undefined } & Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children">);

/** Text-only nav item. Active: surface fill, 600 weight, aria-current="page". */
export function NavItem(props: NavItemProps) {
  const { active, shortcut, platform, children, className, ...rest } = props;
  const inner = (
    <>
      <span className={s.itemLabel} data-text={children}>
        {children}
      </span>
      {shortcut ? (
        <span className={s.itemShortcut} aria-hidden>
          {formatShortcut(shortcut, platform)}
        </span>
      ) : null}
    </>
  );
  const cls = cx(s.item, active && s.active, className);
  if (rest.href !== undefined) {
    return (
      <a className={cls} aria-current={active ? "page" : undefined} {...(rest as AnchorHTMLAttributes<HTMLAnchorElement>)}>
        {inner}
      </a>
    );
  }
  return (
    <button
      type="button"
      className={cls}
      aria-current={active ? "page" : undefined}
      {...(rest as ButtonHTMLAttributes<HTMLButtonElement>)}
    >
      {inner}
    </button>
  );
}

export interface MicStatusProps {
  tone: StatusTone;
  /** "Listening for calls", "On air", "Mic off". */
  label: string;
  className?: string;
}

/** Sidebar mic status: 6 px dot plus mono caps. */
export function MicStatus({ tone, label, className }: MicStatusProps) {
  return (
    <div className={cx(s.mic, className)} role="status">
      <StatusDot tone={tone} label={label} variant="caps" />
    </div>
  );
}
