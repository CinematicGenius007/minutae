// macOS menu-bar panel (window label "tray", route #/tray). A non-activating NSPanel anchored
// under the tray icon; it closes when it loses key (click outside) or after any navigation.
// Windows/Linux never create this window: they keep the native tray menu.
import { Bookmark, Pause, Play } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  Button, formatDuration, formatRelativeDate, RecordPill, Shortcut, Spinner, StatusDot, useToast,
} from "@minutae/ui";
import { setRecordingState, toError, useLevels, useRecording, useSettings } from "../../hooks";
import { api, events } from "../../lib/api";
import type { MeetingListItem, Stage } from "../../lib/types";
import s from "./TrayPanel.module.css";
import { useTransparentWindow } from "./useTransparentWindow";

const RECENT = 3;

const STAGE: Record<Stage, string> = {
  transcribing: "Transcribing",
  diarizing: "Speakers",
  summarizing: "Summarizing",
  indexing: "Indexing",
  downloadingModel: "Downloading",
};

export default function TrayPanelWindow() {
  useTransparentWindow();
  const toast = useToast();
  const { state, elapsedMs, live, capturing, paused } = useRecording();
  const { settings } = useSettings();
  const [recent, setRecent] = useState<MeetingListItem[] | null>(null);
  const [recentError, setRecentError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"record" | "pause" | "mark" | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  const loadRecent = useCallback(() => {
    api
      .listMeetings(RECENT + 1)
      .then((groups) => {
        setRecent(groups.flatMap((g) => g.items).filter((m) => m.status !== "recording").slice(0, RECENT));
        setRecentError(null);
      })
      .catch((e) => setRecentError(toError(e).message));
  }, []);

  useEffect(() => {
    loadRecent();
    const offs: (() => void)[] = [];
    let dead = false;
    const keep = (u: () => void) => (dead ? u() : offs.push(u));
    void events.meetingsChanged(loadRecent).then(keep);
    void events.trayShown(loadRecent).then(keep);
    void events
      .progress((p) => setRecent((prev) => prev?.map((m) => (m.id === p.meetingId ? { ...m, progress: p } : m)) ?? prev))
      .then(keep);
    return () => {
      dead = true;
      for (const u of offs) u();
    };
  }, [loadRecent]);

  // The window is sized to the card: report the height whenever the content changes.
  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const report = () => void api.trayPanelResize(Math.ceil(el.getBoundingClientRect().height)).catch(() => {});
    report();
    const ro = new ResizeObserver(report);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const run = useCallback(
    async (kind: "record" | "pause" | "mark", fn: () => Promise<unknown>, failed: string) => {
      setBusy(kind);
      try {
        await fn();
      } catch (e) {
        toast.show({ title: `${failed} ${toError(e).message}` });
      } finally {
        setBusy(null);
      }
    },
    [toast],
  );

  const record = () =>
    run(
      "record",
      async () => {
        if (live) {
          const id = await api.stopRecording();
          await api.openMain(`/meetings/${id}`);
        } else {
          await api.startRecording();
          const next = await api.recordingState();
          setRecordingState(next);
        }
      },
      live ? "Couldn't stop recording." : "Couldn't start recording.",
    );
  const togglePause = () =>
    run("pause", () => (paused ? api.resumeRecording() : api.pauseRecording()), "Couldn't pause.");
  const mark = () => run("mark", () => api.markMoment(), "Couldn't mark the moment.");

  const listening = settings?.autoDetectMeetings ?? true;

  return (
    <div className={s.stage}>
      <div ref={rootRef} className={s.panel} data-screen="tray" data-ready={String(state !== null && recent !== null)}>
        <section className={s.now} aria-label="Recording">
          {live ? (
            <>
              <StatusDot
                tone={capturing ? "onair" : "off"}
                variant="caps"
                label={capturing ? `On air${state?.sourceApp ? ` · ${state.sourceApp}` : ""}` : paused ? "Paused" : "Saving"}
              />
              <div className={s.nowRow}>
                <div className={s.nowText}>
                  <span className={s.title}>{state?.title ?? "Untitled meeting"}</span>
                  <span className={s.timer} data-tray-timer>{formatDuration(elapsedMs)}</span>
                </div>
                <Levels enabled={capturing} />
              </div>
            </>
          ) : (
            <>
              <StatusDot
                tone={listening ? "ok" : "off"}
                variant="caps"
                label={listening ? "Listening for calls" : "Call detection off"}
              />
              <p className={s.hint}>
                {listening
                  ? "We'll ask before recording when an app uses your mic."
                  : "Turn on call detection in Settings → General."}
              </p>
            </>
          )}
          <div className={s.controls}>
            <RecordPill
              className={s.grow}
              live={live}
              elapsedMs={elapsedMs}
              idleLabel="Record"
              liveLabel="Stop"
              shortcut=""
              showTimer={false}
              fullWidth
              disabled={busy === "record" || state?.phase === "stopping"}
              onClick={record}
            />
            {live && (
              <>
                <Button
                  variant="secondary"
                  icon={paused ? <Play size={16} aria-hidden /> : <Pause size={16} aria-hidden />}
                  loading={busy === "pause"}
                  disabled={state?.phase === "stopping"}
                  onClick={togglePause}
                >
                  {paused ? "Resume" : "Pause"}
                </Button>
                <Button
                  variant="secondary"
                  icon={<Bookmark size={16} aria-hidden />}
                  loading={busy === "mark"}
                  disabled={!capturing}
                  onClick={mark}
                  aria-label="Mark moment"
                >
                  Mark
                </Button>
              </>
            )}
          </div>
        </section>

        <section className={s.recent} aria-labelledby="tray-recent">
          <h2 id="tray-recent" className={s.groupLabel}>Recent</h2>
          {recentError ? (
            <p className={s.empty} role="alert">
              Couldn't load meetings. {recentError}{" "}
              <Button variant="quiet" onClick={loadRecent}>Try again</Button>
            </p>
          ) : recent === null ? (
            <div className={s.empty}><Spinner label="Loading meetings" /></div>
          ) : recent.length === 0 ? (
            <p className={s.empty}>No meetings yet. Record one, or join a call.</p>
          ) : (
            <ul className={s.list}>
              {recent.map((m) => (
                <li key={m.id} className={s.row}>
                  <button type="button" className={s.rowMain} onClick={() => void api.openMain(`/meetings/${m.id}`)}>
                    <span className={s.rowTitle}>{m.title}</span>
                    <span className={s.rowMeta}>
                      {formatRelativeDate(new Date(m.startedAt))}
                      {m.status === "processing"
                        ? ` · ${m.progress ? STAGE[m.progress.stage] : "Processing"}`
                        : m.durationMs
                          ? ` · ${formatDuration(m.durationMs)}`
                          : ""}
                    </span>
                  </button>
                  <span className={s.rowStatus}>
                    {m.status === "failed" ? (
                      <Button variant="quiet" onClick={() => void api.retryMeeting(m.id).then(loadRecent).catch(() => {})}>
                        Retry
                      </Button>
                    ) : m.status === "processing" ? (
                      m.progress ? (
                        <span className={s.meta}>{Math.round(m.progress.fraction * 100)}%</span>
                      ) : (
                        <Spinner label="Processing" />
                      )
                    ) : (
                      <StatusDot tone="ok" label={<span className="mn-sr-only">Done</span>} pulse={false} />
                    )}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </section>

        <nav className={s.footer} aria-label="Minutae">
          {settings && (
            <div className={s.footerRow}>
              <span className={s.hint}>Dictate anywhere</span>
              <span className={s.dictateKey}>
                {settings.dictationMode === "hold" ? "hold " : ""}
                <Shortcut keys={settings.dictationHotkey} variant="caps" size="sm" />
              </span>
            </div>
          )}
          <button type="button" className={s.footerRow} onClick={() => void api.openMain()}>
            <span className={s.footerStrong}>Open Minutae</span>
          </button>
          <div className={s.footerRow}>
            <button type="button" className={s.link} onClick={() => void api.openMain("/settings/general")}>
              Settings…
            </button>
            <button type="button" className={s.linkMuted} onClick={() => void api.quitApp()}>
              Quit
            </button>
          </div>
        </nav>
      </div>
    </div>
  );
}

/** Two thin meters (You = mic, Them = system), driven at ~30 Hz without re-rendering. */
function Levels({ enabled }: { enabled: boolean }) {
  const mic = useRef<HTMLSpanElement>(null);
  const sys = useRef<HTMLSpanElement>(null);
  useLevels((f) => {
    if (mic.current) mic.current.style.transform = `scaleX(${Math.min(1, f.mic)})`;
    if (sys.current) sys.current.style.transform = `scaleX(${Math.min(1, f.system)})`;
  }, enabled);
  return (
    <div className={s.levels} aria-hidden data-tray-levels>
      <span className={s.levelLabel}>You</span>
      <span className={s.meter}><span ref={mic} className={s.meterFill} /></span>
      <span className={s.levelLabel}>Them</span>
      <span className={s.meter}><span ref={sys} className={s.meterFill} /></span>
    </div>
  );
}
