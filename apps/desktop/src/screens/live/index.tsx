// 02 Live recording (Paper D3-0): on-air status, editable title, giant timer, live waveform,
// Stop & summarize / Pause / Mark moment (⌘M); live transcript pane on the right.
import { Bookmark, Pause, Pencil, Play } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button, cx, EmptyState, formatDuration, formatShortcut, localPhrase, useToast } from "@minutae/ui";
import {
  toError,
  useCaptions,
  useGo,
  usePlatform,
  useRecordActions,
  useRecording,
  useSettings,
  useShortcut,
} from "../../hooks";
import { api } from "../../lib/api";
import type { Moment } from "../../lib/types";
import s from "./Live.module.css";
import { LiveTranscript } from "./LiveTranscript";
import { LiveWaveform } from "./LiveWaveform";

function EditableTitle({ meetingId, title }: { meetingId: string | null; title: string }) {
  const toast = useToast();
  const [value, setValue] = useState(title);
  const editing = useRef(false);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!editing.current) setValue(title);
  }, [title]);

  const commit = async () => {
    editing.current = false;
    const next = value.trim();
    if (!next || next === title) {
      setValue(title);
      return;
    }
    if (!meetingId) return;
    try {
      await api.renameMeeting(meetingId, next);
    } catch (e) {
      setValue(title);
      toast.show({ title: `Couldn't rename. ${toError(e).message}` });
    }
  };

  return (
    <div className={s.titleRow}>
      <span className={s.titleSizer} data-value={value || "Untitled meeting"}>
        <input
          ref={input}
          className={s.titleInput}
          size={1}
          value={value}
          placeholder="Untitled meeting"
          aria-label="Meeting title"
          onFocus={() => (editing.current = true)}
          onChange={(e) => setValue(e.currentTarget.value)}
          onBlur={() => void commit()}
          onKeyDown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur();
            if (e.key === "Escape") {
              setValue(title);
              editing.current = false;
              requestAnimationFrame(() => input.current?.blur());
            }
          }}
        />
      </span>
      <button type="button" className={s.edit} onClick={() => input.current?.select()} aria-label="Rename meeting">
        <Pencil size={16} strokeWidth={2} aria-hidden />
      </button>
    </div>
  );
}

export default function LiveScreen() {
  const go = useGo();
  const toast = useToast();
  const platform = usePlatform();
  const { update: updateSettings } = useSettings();
  const { state, elapsedMs, live, capturing, paused } = useRecording();
  const { start, stop, busy } = useRecordActions();
  const captions = useCaptions(state?.meetingId ?? null);
  const [moments, setMoments] = useState<Moment[]>([]);
  const [pausing, setPausing] = useState(false);

  // Reset marked moments per recording.
  const meetingId = state?.meetingId ?? null;
  useEffect(() => setMoments([]), [meetingId]);

  // If the backend stops the recording (call ended, tray), follow it to the meeting.
  const lastId = useRef<string | null>(null);
  const stoppingHere = useRef(false);
  useEffect(() => {
    if (live && meetingId) lastId.current = meetingId;
    if (state?.phase === "idle" && lastId.current && !stoppingHere.current) {
      const id = lastId.current;
      lastId.current = null;
      go(`/meetings/${id}`);
    }
  }, [live, meetingId, state?.phase, go]);

  const mark = async () => {
    if (!live) return;
    try {
      const m = await api.markMoment();
      setMoments((prev) => [...prev, m]);
      toast.show({ title: `Moment marked at ${formatDuration(m.atMs)}` });
    } catch (e) {
      toast.show({ title: `Couldn't mark the moment. ${toError(e).message}` });
    }
  };
  useShortcut("mod+m", () => void mark(), { enabled: live });

  const togglePause = async () => {
    setPausing(true);
    try {
      if (paused) await api.resumeRecording();
      else await api.pauseRecording();
    } catch (e) {
      toast.show({ title: `Couldn't ${paused ? "resume" : "pause"}. ${toError(e).message}` });
    } finally {
      setPausing(false);
    }
  };

  const onStop = async () => {
    stoppingHere.current = true;
    lastId.current = null;
    await stop();
    stoppingHere.current = false;
  };

  if (!state) return null;

  if (!live) {
    return (
      <div className={s.idle} data-screen="live" data-ready="true">
        <EmptyState
          title="Not recording"
          description={`Join a call and Minutae will ask, or press ${formatShortcut("mod+r", platform)}. Audio stays ${localPhrase(platform)}.`}
          action={
            <Button onClick={() => void start()} loading={busy} shortcut="mod+r" platform={platform}>
              Record
            </Button>
          }
        />
      </div>
    );
  }

  const sources =
    state.systemAudio === "on" ? "Mic + system audio" : "Mic only";
  const stopping = state.phase === "stopping";

  return (
    <div className={s.screen} data-screen="live" data-ready="true">
      <section className={s.capture} aria-label="Recording">
        <div className={s.status}>
          <div className={s.statusLine} role="status">
            <span className={cx(s.dot, !capturing && s.dotOff)} aria-hidden />
            {capturing ? (
              <span className={s.onair}>On air</span>
            ) : (
              <span className={s.state}>{stopping ? "Saving" : "Paused"}</span>
            )}
            <span>· {sources} · Saved locally</span>
          </div>
          {state.systemAudio === "unavailable" ? (
            <p className={s.note}>
              System audio isn't available{state.systemAudioError ? `: ${state.systemAudioError}` : ""}. Recording your mic only.
            </p>
          ) : null}
        </div>

        <div className={s.hero}>
          <EditableTitle meetingId={meetingId} title={state.title ?? ""} />
          <p className={cx(s.timer, !capturing && s.timerPaused)} role="timer" aria-live="off">
            {formatDuration(elapsedMs)}
          </p>
          <LiveWaveform capturing={capturing} />
        </div>

        <div className={s.controls}>
          <Button size="hero" icon={<span className={s.stopGlyph} />} onClick={() => void onStop()} loading={busy || stopping}>
            Stop &amp; summarize
          </Button>
          <Button
            size="hero"
            variant="secondary"
            icon={paused ? <Play size={14} strokeWidth={2} fill="currentColor" /> : <Pause size={14} strokeWidth={2} fill="currentColor" />}
            onClick={() => void togglePause()}
            disabled={pausing || stopping}
          >
            {paused ? "Resume" : "Pause"}
          </Button>
          <Button
            size="hero"
            variant="secondary"
            icon={<Bookmark size={14} strokeWidth={2.2} />}
            shortcut="mod+m"
            platform={platform}
            onClick={() => void mark()}
            disabled={stopping}
          >
            Mark moment
          </Button>
        </div>
      </section>

      <LiveTranscript
        captions={captions}
        moments={moments}
        where="On-device"
        status={state.captions}
        paused={paused}
        onTurnOn={() => {
          updateSettings({ liveCaptions: true }).catch((e) =>
            toast.show({ title: `Couldn't turn on captions. ${toError(e).message}` }),
          );
        }}
        onGetModel={() => go("/settings/transcription")}
      />
    </div>
  );
}
