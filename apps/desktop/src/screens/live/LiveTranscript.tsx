// Right pane of the live view: captions from `recording://caption` and marked moments.
import { Bookmark } from "lucide-react";
import { useEffect, useLayoutEffect, useRef } from "react";
import { cx, formatDuration } from "@minutae/ui";
import type { CaptionLine, Moment } from "../../lib/types";
import s from "./Live.module.css";

type Item = { kind: "caption"; at: number; line: CaptionLine } | { kind: "moment"; at: number; moment: Moment };

export function LiveTranscript({
  captions,
  moments,
  where,
}: {
  captions: CaptionLine[];
  moments: Moment[];
  where: string;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const items: Item[] = [
    ...captions.map((line) => ({ kind: "caption" as const, at: line.atMs, line })),
    ...moments.map((moment) => ({ kind: "moment" as const, at: moment.atMs, moment })),
  ].sort((a, b) => a.at - b.at);
  const lastCaptions = new Set(captions.slice(-2));

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const onScroll = () => {
      pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, []);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  });

  return (
    <aside className={s.pane} aria-label="Live transcript">
      <div className={s.paneHead}>
        <h2 className={s.paneTitle}>Live transcript</h2>
        <span className={s.paneMeta}>{where}</span>
      </div>
      <div ref={scroller} className={s.feed} aria-live="polite" aria-relevant="additions">
        {items.map((it) =>
          it.kind === "moment" ? (
            <div key={`m${it.at}`} className={s.marker}>
              <Bookmark size={12} strokeWidth={2} fill="currentColor" aria-hidden />
              <span>
                Marked at {formatDuration(it.at)}
                {it.moment.label && it.moment.label !== "Moment" ? ` — “${it.moment.label}”` : ""}
              </span>
            </div>
          ) : (
            <div key={`c${it.at}${it.line.speaker}`} className={s.line}>
              <span className={s.lineMeta}>
                {formatDuration(it.line.atMs)} · <span className={s.speaker}>{it.line.speaker}</span>
              </span>
              <p className={cx(s.lineText, !lastCaptions.has(it.line) && s.lineOld)}>
                {it.line.text}
                {!it.line.final ? <span className={s.partial}>…</span> : null}
              </p>
            </div>
          ),
        )}
        {captions.length === 0 ? (
          <p className={s.placeholder}>Live captions are off — the full transcript appears after the call.</p>
        ) : null}
      </div>
    </aside>
  );
}
