// Meeting detail panes: summary, processing / failed states, transcript, ask, right rail.
import { useMemo, useRef, useState } from "react";
import {
  Button,
  Checkbox,
  formatDuration,
  GroupLabel,
  localPhrase,
  type Platform,
  Progress,
  Spinner,
  TextField,
  useToast,
} from "@minutae/ui";
import { toError, useFailureFix, useGo } from "../../hooks";
import { api } from "../../lib/api";
import type { ActionItem, Answer, Failure, MeetingDetail, Moment, Progress as JobProgress, Segment, Speaker } from "../../lib/types";
import s from "./Detail.module.css";

/* ── Summary ─────────────────────────────────────────────────────────────── */

export function SummaryPane({ detail }: { detail: MeetingDetail }) {
  const summary = detail.summary;
  if (!summary) return null;
  return (
    <div className={s.content}>
      <p className={s.lead}>{summary.overview}</p>
      {summary.decisions.length ? (
        <section className={s.section}>
          <h3 className={s.label}>Decisions</h3>
          <ol className={s.numbered}>
            {summary.decisions.map((d, i) => (
              <li key={d}>
                <span className={s.num} aria-hidden>
                  {String(i + 1).padStart(2, "0")}
                </span>
                <span className={s.reading17}>{d}</span>
              </li>
            ))}
          </ol>
        </section>
      ) : null}
      {summary.points.length ? (
        <section className={s.section}>
          <h3 className={s.label}>Points</h3>
          <ul className={s.numbered}>
            {summary.points.map((p) => (
              <li key={p}>
                <span className={s.num} aria-hidden>
                  —
                </span>
                <span className={s.reading17}>{p}</span>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
      <p className={s.provenance}>
        Summarized by {summary.provider}
        {summary.model ? ` · ${summary.model}` : ""}
      </p>
    </div>
  );
}

/* ── Processing / failed ────────────────────────────────────────────────── */

export function stageCopy(p: JobProgress, platform: Platform): string {
  const where = localPhrase(platform);
  const label = {
    transcribing: `Transcribing ${where}`,
    diarizing: "Telling speakers apart",
    summarizing: "Writing the summary",
    indexing: "Indexing for search",
    downloadingModel: "Downloading the speech model",
  }[p.stage];
  const pct = `${Math.round(p.fraction * 100)}%`;
  const eta = p.etaSeconds != null && p.etaSeconds > 0 ? ` · ~${Math.max(1, Math.round(p.etaSeconds / 60))} min left` : "";
  return `${label} · ${pct}${eta}`;
}

export function ProcessingPanel({ progress, platform }: { progress: JobProgress | null; platform: Platform }) {
  return (
    <div className={s.statePanel}>
      <h3 className={s.stateTitle}>Working on the summary</h3>
      <p className={s.stateBody}>You can leave this page. Minutae lets you know when it's ready.</p>
      {progress ? (
        <Progress className={s.processing} value={progress.fraction} caption={stageCopy(progress, platform)} />
      ) : (
        <div className={s.thinking}>
          <Spinner /> Waiting to start
        </div>
      )}
    </div>
  );
}

export function FailedPanel({ meetingId, failure, onRetried }: { meetingId: string; failure: Failure | null; onRetried: () => void }) {
  const fixFailure = useFailureFix();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const retry = async () => {
    setBusy(true);
    try {
      await api.retryMeeting(meetingId);
      onRetried();
    } catch (e) {
      toast.show({ title: `Couldn't retry. ${toError(e).message}` });
    } finally {
      setBusy(false);
    }
  };
  const fix = () => fixFailure(failure?.kind, () => void retry());
  return (
    <div className={s.statePanel} role="alert">
      <h3 className={`${s.stateTitle} ${s.stateTitleFailed}`}>Not summarized</h3>
      <p className={s.stateBody}>{failure?.message ?? "The summary didn't finish. Audio and transcript are saved."}</p>
      <div className={s.stateActions}>
        {failure?.fix ? (
          <Button variant="secondary" onClick={fix}>
            {failure.fix}
          </Button>
        ) : null}
        <Button variant="quiet" onClick={() => void retry()} loading={busy}>
          Retry
        </Button>
      </div>
    </div>
  );
}

/* ── Transcript ─────────────────────────────────────────────────────────── */

export function speakerName(sp: Speaker | undefined, track: Segment["track"]): string {
  if (!sp) return track === "mic" ? "You" : "Speaker";
  return sp.displayName ?? (sp.track === "mic" ? "You" : sp.label);
}

function SpeakerLabel({ speaker, name, onRenamed }: { speaker: Speaker | undefined; name: string; onRenamed: () => void }) {
  const toast = useToast();
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(name);
  const cancelled = useRef(false);
  const restoreFocus = useRef(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  if (!speaker) return <span className={s.speaker}>{name}</span>;
  if (!editing) {
    return (
      <button
        ref={btnRef}
        type="button"
        className={s.speaker}
        title="Rename speaker"
        aria-label={`${name}. Rename speaker`}
        onClick={() => {
          setValue(speaker.displayName ?? (speaker.track === "mic" ? "You" : ""));
          cancelled.current = false;
          restoreFocus.current = false;
          setEditing(true);
        }}
      >
        {name}
      </button>
    );
  }
  const commit = async () => {
    setEditing(false);
    if (restoreFocus.current) {
      restoreFocus.current = false;
      requestAnimationFrame(() => btnRef.current?.focus());
    }
    const next = value.trim();
    if (cancelled.current || !next || next === name) return;
    try {
      await api.renameSpeaker(speaker.id, next);
      onRenamed();
    } catch (e) {
      toast.show({ title: `Couldn't rename. ${toError(e).message}` });
    }
  };
  return (
    <input
      className={s.speakerInput}
      // biome-ignore lint/a11y/noAutofocus: opened by an explicit click
      autoFocus
      value={value}
      placeholder={speaker.label}
      aria-label={`Name for ${speaker.label}`}
      onChange={(e) => setValue(e.currentTarget.value)}
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
  );
}

export function TranscriptPane({
  detail,
  onSeek,
  onRenamed,
}: {
  detail: MeetingDetail;
  onSeek: (ms: number) => void;
  onRenamed: () => void;
}) {
  const speakers = useMemo(() => new Map(detail.speakers.map((sp) => [sp.id, sp])), [detail.speakers]);
  const turns = useMemo(() => {
    const out: { key: string; speakerId: string | null; track: Segment["track"]; startMs: number; segs: Segment[] }[] = [];
    for (const seg of detail.segments) {
      const last = out[out.length - 1];
      if (last && last.speakerId === seg.speakerId && last.track === seg.track) last.segs.push(seg);
      else out.push({ key: `${seg.startMs}`, speakerId: seg.speakerId, track: seg.track, startMs: seg.startMs, segs: [seg] });
    }
    return out;
  }, [detail.segments]);

  if (!turns.length) {
    return (
      <div className={s.statePanel}>
        <p className={s.stateBody}>
          {detail.meeting.status === "processing" ? "The transcript appears here when transcribing finishes." : "No transcript for this meeting."}
        </p>
      </div>
    );
  }
  return (
    <div className={s.transcript}>
      {turns.map((t) => {
        const sp = t.speakerId ? speakers.get(t.speakerId) : undefined;
        const name = speakerName(sp, t.track);
        return (
          <div key={t.key} className={s.turn}>
            <div className={s.turnHead}>
              <SpeakerLabel speaker={sp} name={name} onRenamed={onRenamed} />
              <button type="button" className={s.stamp} onClick={() => onSeek(t.startMs)} aria-label={`Play from ${formatDuration(t.startMs)}`}>
                {formatDuration(t.startMs)}
              </button>
            </div>
            <div className={s.turnText}>
              {t.segs.map((seg) => (
                <p key={seg.startMs}>{seg.text}</p>
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}

/* ── Ask ─────────────────────────────────────────────────────────────────── */

export function AskPane({ detail, onSeek }: { detail: MeetingDetail; onSeek: (ms: number) => void }) {
  const go = useGo();
  const [q, setQ] = useState("");
  const [asked, setAsked] = useState("");
  const [answer, setAnswer] = useState<Answer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const ask = async (question: string) => {
    if (!question.trim()) return;
    setBusy(true);
    setError(null);
    setAsked(question);
    try {
      // The backend's ask is global; scope it by naming the meeting.
      setAnswer(await api.ask(`In the meeting "${detail.meeting.title}": ${question.trim()}`));
    } catch (e) {
      setAnswer(null);
      setError(toError(e).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={s.ask}>
      <form
        className={s.askForm}
        onSubmit={(e) => {
          e.preventDefault();
          void ask(q);
        }}
      >
        <TextField
          className={s.askField}
          aria-label="Ask about this meeting"
          placeholder="Ask about this meeting"
          value={q}
          onChange={(e) => setQ(e.currentTarget.value)}
          help="Answers come from this meeting's transcript."
        />
        <Button type="submit" variant="secondary" loading={busy} disabled={!q.trim()}>
          Ask
        </Button>
      </form>
      {busy ? (
        <div className={s.thinking} role="status">
          <Spinner /> Reading the transcript
        </div>
      ) : error ? (
        <div className={s.statePanel} role="alert">
          <p className={s.stateBody}>Couldn't answer that. {error}</p>
          <div className={s.stateActions}>
            <Button variant="quiet" onClick={() => void ask(asked)}>
              Retry
            </Button>
          </div>
        </div>
      ) : answer ? (
        <div className={s.answer}>
          <p className={s.reading17}>{answer.text}</p>
          {answer.sources.length ? (
            <section className={s.section}>
              <GroupLabel as="h3">Sources</GroupLabel>
              <div className={s.sources}>
                {answer.sources.map((h) => (
                  <button
                    key={`${h.meetingId}${h.dictationId}${h.atMs}`}
                    type="button"
                    className={s.source}
                    onClick={() => {
                      if (h.meetingId === detail.meeting.id && h.atMs != null) onSeek(h.atMs);
                      else if (h.meetingId) go(`/meetings/${h.meetingId}`);
                    }}
                  >
                    <span className={s.sourceTitle}>{h.title}</span>
                    <span className={s.sourceSnippet}>{h.snippet}</span>
                    {h.atMs != null ? <span className={s.sourceMeta}>{formatDuration(h.atMs)}</span> : null}
                  </button>
                ))}
              </div>
            </section>
          ) : null}
          <p className={s.provenance}>Answered by {answer.provider}</p>
        </div>
      ) : null}
    </div>
  );
}

/* ── Rail ────────────────────────────────────────────────────────────────── */

export function ActionItems({ items, onToggle }: { items: ActionItem[]; onToggle: (id: string, done: boolean) => void }) {
  const done = items.filter((i) => i.done).length;
  return (
    <section className={s.items} aria-label="Action items">
      <GroupLabel as="h2" count={items.length ? `${done} / ${items.length}` : undefined}>
        Action items
      </GroupLabel>
      {items.length === 0 ? <p className={s.railEmpty}>No action items.</p> : null}
      {items.map((it) => {
        const id = `ai-${it.id}`;
        const meta = [it.owner, it.done ? "Done" : (it.due ?? "No date")].filter(Boolean).join(" · ");
        return (
          <div key={it.id} className={s.item}>
            <Checkbox id={id} checked={it.done} onCheckedChange={(v) => onToggle(it.id, v === true)} />
            <label htmlFor={id} className={s.itemText}>
              <span className={`${s.itemTitle} ${it.done ? s.itemDone : ""}`}>{it.text}</span>
              <span className={s.itemMeta}>{meta}</span>
            </label>
          </div>
        );
      })}
    </section>
  );
}

export function Moments({ moments, onSeek }: { moments: Moment[]; onSeek: (ms: number) => void }) {
  if (!moments.length) return null;
  return (
    <section className={s.section} aria-label="Moments">
      <h2 className={s.label}>Moments</h2>
      <ul className={s.moments}>
        {moments.map((m) => (
          <li key={m.atMs} className={s.moment}>
            <button type="button" className={s.momentTime} onClick={() => onSeek(m.atMs)} aria-label={`Play from ${formatDuration(m.atMs)}`}>
              {formatDuration(m.atMs)}
            </button>
            <span className={s.momentLabel}>{m.label}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}
