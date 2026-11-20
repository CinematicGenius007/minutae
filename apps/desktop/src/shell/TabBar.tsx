// Narrow-width navigation (< 900 px): four places with a centre Record button (Paper "07 Mobile").
import { AudioLines, Library, Settings, TextAlignStart } from "lucide-react";
import { cx } from "@minutae/ui";
import s from "./Shell.module.css";

export interface Place {
  href: string;
  label: string;
  active: boolean;
}

const ICONS = { Meetings: TextAlignStart, Dictate: AudioLines, Library, Settings } as const;

export function TabBar({
  places,
  live,
  onRecord,
}: {
  places: Place[];
  live: boolean;
  onRecord: () => void;
}) {
  const tab = (p: Place) => {
    const Icon = ICONS[p.label as keyof typeof ICONS];
    return (
      <a key={p.href} href={`#${p.href}`} className={cx(s.tab, p.active && s.tabActive)} aria-current={p.active ? "page" : undefined}>
        <Icon size={20} strokeWidth={2} aria-hidden />
        {p.label}
      </a>
    );
  };
  return (
    <nav className={s.tabbar} aria-label="Main">
      {places.slice(0, 2).map(tab)}
      <button
        type="button"
        className={cx(s.recordFab, live && s.recordFabLive)}
        onClick={onRecord}
        aria-label={live ? "Recording. Open the live view" : "Record"}
      >
        <span className={live ? s.recordFabStop : s.recordFabDot} aria-hidden />
      </button>
      {places.slice(2).map(tab)}
    </nav>
  );
}
