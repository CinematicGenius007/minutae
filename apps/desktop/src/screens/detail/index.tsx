// 03 Meeting detail (Paper IS-0): back link + toolbar, overline, title (inline rename),
// participants, Summary / Transcript / Ask tabs, action items + moments rail, player bar.
import { ChevronLeft } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useParams } from "wouter";
import {
  Button,
  EmptyState,
  formatClock,
  PlayerBar,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  useToast,
} from "@minutae/ui";
import { toError, useEvent, usePlatform, useQuery, useShortcut } from "../../hooks";
import { api, events } from "../../lib/api";
import type { MeetingDetail, Progress as JobProgress } from "../../lib/types";
import s from "./Detail.module.css";
import { ActionItems, AskPane, FailedPanel, Moments, ProcessingPanel, SummaryPane, TranscriptPane } from "./parts";
import { useAudioPlayer } from "./useAudioPlayer";

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function minutes(ms: number): string {
  const m = Math.max(1, Math.round(ms / 60_000));
  return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${m % 60} min`;
}

function overline(d: MeetingDetail): string {
  const at = new Date(d.meeting.startedAt);
  const parts = [`${WEEKDAYS[at.getDay()]} ${at.getDate()} ${MONTHS[at.getMonth()]}`, formatClock(at)];
  if (d.durationMs) parts.push(minutes(d.durationMs));
  if (d.meeting.sourceApp) parts.push(d.meeting.sourceApp);
  return parts.join(" · ");
}

function EditableTitle({ id, title, onRenamed }: { id: string; title: string; onRenamed: (t: string) => void }) {
  const toast = useToast();
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(title);
  const cancelled = useRef(false);
  const restoreFocus = useRef(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const commit = async () => {
    setEditing(false);
    if (restoreFocus.current) {
      restoreFocus.current = false;
      requestAnimationFrame(() => btnRef.current?.focus());
    }
    const next = value.trim();
    if (cancelled.current || !next || next === title) return;
    onRenamed(next);
    try {
      await api.renameMeeting(id, next);
    } catch (e) {
      onRenamed(title);
      toast.show({ title: `Couldn't rename. ${toError(e).message}` });
    }
  };
  return (
    <h1 className={s.title}>
      {editing ? (
        <input
          className={s.titleInput}
          // biome-ignore lint/a11y/noAutofocus: opened by an explicit click
          autoFocus
          value={value}
          aria-label="Meeting title"
          onChange={(e) => setValue(e.currentTarget.value)}
          onFocus={(e) => e.currentTarget.select()}
          onBlur={() => void commit()}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              restoreFocus.current = true;
              e.currentTarget.blur();
            }
            if (e.key === "Escape") {
              cancelled.current = true;
              restoreFocus.current = true;
              e.currentTarget.blur();
            }
          }}
        />
      ) : (
        <button
          ref={btnRef}
          type="button"
          className={s.titleButton}
          title="Rename"
          onClick={() => {
            cancelled.current = false;
            restoreFocus.current = false;
            setValue(title);
            setEditing(true);
          }}
        >
          {title}
        </button>
      )}
    </h1>
  );
}

function Toolbar({ detail }: { detail: MeetingDetail | undefined }) {
  const toast = useToast();
  const platform = usePlatform();
  const md = detail?.summary?.markdownPath ?? null;
  const reveal = platform === "mac" ? "Show in Finder" : platform === "windows" ? "Show in Explorer" : "Show in folder";
  const run = (fn: () => Promise<void>, ok?: string) => async () => {
    try {
      await fn();
      if (ok) toast.show({ title: ok });
    } catch (e) {
      toast.show({ title: toError(e).message });
    }
  };
  return (
    <div className={s.toolbar}>
      <a className={s.back} href="#/">
        <ChevronLeft size={14} strokeWidth={2.2} aria-hidden />
        Meetings
      </a>
      {detail?.summary ? (
        <div className={s.actions}>
          {md ? (
            <>
              <Button variant="quiet" onClick={run(() => api.openPath(md))}>
                Open .md
              </Button>
              <Button variant="quiet" onClick={run(() => api.revealPath(md))}>
                {reveal}
              </Button>
            </>
          ) : null}
          <Button onClick={run(() => api.copyText(detail.summary?.markdown ?? ""), "Summary copied")}>Copy summary</Button>
        </div>
      ) : null}
    </div>
  );
}

export default function DetailScreen() {
  const { id = "" } = useParams<{ id: string }>();
  const platform = usePlatform();
  const toast = useToast();
  const q = useQuery(() => api.getMeeting(id), [id], { refetchOn: [events.meetingsChanged] });
  const detail = q.data?.meeting.id === id ? q.data : undefined;
  const [tab, setTab] = useState("summary");
  const [live, setLive] = useState<JobProgress | null>(null);

  useEffect(() => {
    setTab("summary");
    setLive(null);
  }, [id]);

  useEvent(events.progress, (p: JobProgress) => {
    if (p.meetingId !== id) return;
    if (p.fraction >= 1) {
      setLive(null);
      void q.refetch();
    } else setLive(p);
  });

  const player = useAudioPlayer(id, detail?.audio ?? null, detail?.durationMs ?? 0);
  useShortcut("space", player.toggle, { enabled: player.available });
  const seek = (ms: number) => player.seek(ms, true);

  const toggleAction = async (itemId: string, done: boolean) => {
    const patch = (v: boolean) =>
      q.setData((prev) => prev && { ...prev, actionItems: prev.actionItems.map((a) => (a.id === itemId ? { ...a, done: v } : a)) });
    patch(done);
    try {
      await api.setActionDone(itemId, done);
    } catch (e) {
      patch(!done);
      toast.show({ title: `Couldn't update the action item. ${toError(e).message}` });
    }
  };

  if (q.error && !detail) {
    return (
      <div className={s.screen} data-screen="detail" data-ready={String(!!detail || !!q.error)}>
        <Toolbar detail={undefined} />
        <div className={s.body}>
          <EmptyState
            title="Couldn't open this meeting."
            description={q.error.message}
            action={
              <Button variant="secondary" onClick={() => void q.refetch()}>
                Try again
              </Button>
            }
          />
        </div>
      </div>
    );
  }

  const progress = live ?? detail?.progress ?? null;
  const processing = !!detail && (detail.meeting.status === "processing" || !!live);
  const failed = detail?.meeting.status === "failed";
  const people = detail
    ? detail.speakers
        .filter((sp) => sp.track === "system")
        .map((sp) => sp.displayName ?? sp.label)
        .join(", ")
    : "";

  return (
    <div className={s.screen} data-screen="detail" data-ready={String(!!detail || !!q.error)}>
      <Toolbar detail={detail} />
      <div className={s.body} aria-busy={!detail || undefined}>
        <div className={s.reading}>
          <header className={s.header}>
            {detail ? (
              <>
                <div className={s.overline}>{overline(detail)}</div>
                <EditableTitle
                  id={id}
                  title={detail.meeting.title}
                  onRenamed={(t) => q.setData((prev) => prev && { ...prev, meeting: { ...prev.meeting, title: t } })}
                />
                {people ? <div className={s.people}>{people}</div> : null}
              </>
            ) : (
              <>
                <div className={`${s.skel} ${s.skelOverline}`} />
                <div className={`${s.skel} ${s.skelTitle}`} />
                <div className={`${s.skel} ${s.skelPeople}`} />
              </>
            )}
          </header>

          <Tabs value={tab} onValueChange={setTab}>
            <TabsList aria-label="Meeting views">
              <TabsTrigger value="summary">Summary</TabsTrigger>
              <TabsTrigger value="transcript">Transcript</TabsTrigger>
              <TabsTrigger value="ask">Ask</TabsTrigger>
            </TabsList>
            {detail ? (
              <>
                <TabsContent value="summary" className={s.tabPanel}>
                  <div className={s.summaryStack}>
                    {processing || (!detail.summary && !failed) ? (
                      <ProcessingPanel progress={progress} platform={platform} />
                    ) : null}
                    {failed && !processing ? (
                      <FailedPanel meetingId={id} failure={detail.meeting.failure} onRetried={() => void q.refetch()} />
                    ) : null}
                    {detail.summary ? <SummaryPane detail={detail} /> : null}
                  </div>
                </TabsContent>
                <TabsContent value="transcript" className={s.tabPanel}>
                  <TranscriptPane detail={detail} onSeek={seek} onRenamed={() => void q.refetch()} />
                </TabsContent>
                <TabsContent value="ask" className={s.tabPanel}>
                  <AskPane detail={detail} onSeek={seek} />
                </TabsContent>
              </>
            ) : null}
          </Tabs>
        </div>

        {detail ? (
          <aside className={s.rail} aria-label="Action items and moments">
            <ActionItems items={detail.actionItems} onToggle={(i, d) => void toggleAction(i, d)} />
            <Moments moments={detail.moments} onSeek={seek} />
          </aside>
        ) : null}
      </div>

      <PlayerBar
        className={s.player}
        playing={player.playing}
        onTogglePlay={player.toggle}
        currentMs={player.currentMs}
        durationMs={player.durationMs}
        onSeek={(ms) => player.seek(ms)}
        speed={player.speed}
        onSpeedChange={player.setSpeed}
        disabled={!player.available}
      />
    </div>
  );
}
