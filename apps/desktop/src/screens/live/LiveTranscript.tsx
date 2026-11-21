// Right pane of the live view: captions from `recording://caption` and marked moments.
// Follows new lines while scrolled to the bottom; scrolling up stops that until the user
// scrolls back down or presses "Jump to latest".
import { ArrowDown, Bookmark } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Button, cx, formatDuration } from "@minutae/ui";
import type { CaptionLine, CaptionStatus, Moment } from "../../lib/types";
import s from "./Live.module.css";

type Item = { kind: "caption"; at: number; line: CaptionLine } | { kind: "moment"; at: number; moment: Moment };

/** Lines shown at full contrast; older ones are muted. */
const RECENT = 3;

export function LiveTranscript({
  captions,
  moments,
  where,
  status,
  paused,
  onTurnOn,
  onGetModel,
}: {
  captions: CaptionLine[];
  moments: Moment[];
  where: string;
  status: CaptionStatus;
  paused: boolean;
  onTurnOn: () => void;
  onGetModel: () => void;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const [following, setFollowing] = useState(true);
  const items: Item[] = [
    ...captions.map((line) => ({ kind: "caption" as const, at: line.atMs, line })),
    ...moments.map((moment) => ({ kind: "moment" as const, at: moment.atMs, moment })),
  ].sort((a, b) => a.at - b.at);
  const recent = new Set(captions.slice(-RECENT).map((l) => l.id));

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const onScroll = () => {
      pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
      setFollowing(pinned.current);
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, []);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  });

  const jump = () => {
    const el = scroller.current;
    if (!el) return;
    pinned.current = true;
    setFollowing(true);
    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  };

  const meta = status === "on" && paused ? `${where} · Paused` : where;

  return (
    <aside className={s.pane} aria-label="Live transcript">
      <div className={s.paneHead}>
        <h2 className={s.paneTitle}>Live transcript</h2>
        <span className={s.paneMeta}>{meta}</span>
      </div>
      <div ref={scroller} className={s.feed} aria-live="polite" aria-relevant="additions" data-testid="captions">
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
            <div key={`c${it.line.id}`} className={s.line} data-final={String(it.line.final)}>
              <span className={s.lineMeta}>
                {formatDuration(it.line.atMs)} · <span className={s.speaker}>{it.line.speaker}</span>
              </span>
              <p className={cx(s.lineText, !recent.has(it.line.id) && s.lineOld)}>
                {it.line.text}
                {!it.line.final ? <span className={s.partial}>…</span> : null}
              </p>
            </div>
          ),
        )}
        {captions.length === 0 ? <Placeholder status={status} onTurnOn={onTurnOn} onGetModel={onGetModel} /> : null}
      </div>
      {!following && captions.length > 0 ? (
        <div className={s.jump}>
          <Button variant="secondary" icon={<ArrowDown size={14} strokeWidth={2} />} onClick={jump}>
            Jump to latest
          </Button>
        </div>
      ) : null}
    </aside>
  );
}

function Placeholder({
  status,
  onTurnOn,
  onGetModel,
}: {
  status: CaptionStatus;
  onTurnOn: () => void;
  onGetModel: () => void;
}) {
  if (status === "noModel") {
    return (
      <div className={s.placeholder}>
        <p>Live captions need a speech model. The full transcript still appears after the call.</p>
        <Button variant="quiet" className={s.placeholderAction} onClick={onGetModel}>
          Get a model
        </Button>
      </div>
    );
  }
  if (status === "off") {
    return (
      <div className={s.placeholder}>
        <p>Live captions are off. The full transcript appears after the call.</p>
        <Button variant="quiet" className={s.placeholderAction} onClick={onTurnOn}>
          Turn on captions
        </Button>
      </div>
    );
  }
  return (
    <div className={s.placeholder}>
      <p>Captions appear here as people talk.</p>
    </div>
  );
}
