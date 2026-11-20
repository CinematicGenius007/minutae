import { Button, GroupLabel, Progress, SettingRow, Spinner, formatBytes, localPhrase, usePlatform } from "@minutae/ui";
import { useEffect, useState } from "react";
import { api, events } from "../../../lib/api";
import type { ImportProgress, ImportResult, LegacySource } from "../../../lib/types";
import { errorText, useLoad } from "../../../hooks";
import { tildify } from "../shared/format";
import p from "../shared/page.module.css";
import s from "../settings.module.css";

type Run =
  | { phase: "idle" }
  | { phase: "running"; progress: ImportProgress | null }
  | { phase: "done"; result: ImportResult }
  | { phase: "error"; message: string };

const MAX_ISSUES = 5;

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

function sourceLine(src: LegacySource): string {
  if (src.error) return `${src.error} It will be skipped.`;
  const parts = [plural(src.meetings, "meeting")];
  if (src.dictations > 0) parts.push(plural(src.dictations, "dictation"));
  if (src.alreadyImported > 0) parts.push(`${src.alreadyImported} already imported`);
  return parts.join(" · ");
}

const remainingOf = (sources: LegacySource[]) =>
  sources.filter((x) => !x.error).reduce((n, x) => n + Math.max(0, x.meetings + x.dictations - x.alreadyImported), 0);

function resultTitle(r: ImportResult): string {
  if (r.imported === 0 && r.dictations === 0) return "Nothing new to import";
  const parts = [];
  if (r.imported > 0) parts.push(plural(r.imported, "meeting"));
  if (r.dictations > 0) parts.push(plural(r.dictations, "dictation"));
  return `Imported ${parts.join(" and ")}`;
}

function resultLine(r: ImportResult): string {
  const parts = [];
  if (r.copiedBytes > 0) parts.push(`${formatBytes(r.copiedBytes)} of audio copied`);
  if (r.skipped > 0) parts.push(`${r.skipped} already there`);
  if (r.failed > 0) parts.push(`${r.failed} couldn't be imported`);
  return parts.join(" · ") || "Done.";
}

/** Settings → Storage: bring meetings over from the Swift app and the Tauri v1 app (MIG-10). */
export function ImportLegacySection() {
  const platform = usePlatform();
  const [scan, rescan] = useLoad(() => api.importLegacyScan());
  const [run, setRun] = useState<Run>({ phase: "idle" });

  useEffect(() => {
    const offs: (() => void)[] = [];
    let dead = false;
    const keep = (u: () => void) => (dead ? u() : offs.push(u));
    void events.importProgress((progress) => setRun({ phase: "running", progress })).then(keep);
    void events
      .importFinished((f) => {
        setRun(f.result ? { phase: "done", result: f.result } : { phase: "error", message: f.error ?? "The import stopped." });
        void rescan();
      })
      .then(keep);
    return () => {
      dead = true;
      for (const off of offs) off();
    };
  }, [rescan]);

  // Re-opened Settings while an import started earlier is still going.
  const runningElsewhere = scan.data?.running ?? false;
  useEffect(() => {
    if (runningElsewhere) setRun((r) => (r.phase === "idle" ? { phase: "running", progress: null } : r));
  }, [runningElsewhere]);

  const start = () => {
    setRun({ phase: "running", progress: null });
    api.importLegacyRun().catch((e) => setRun({ phase: "error", message: errorText(e) }));
  };

  const sources = scan.data?.sources ?? [];
  const usable = sources.filter((x) => !x.error);
  const remaining = remainingOf(sources);
  const audio = usable.reduce((n, x) => n + x.audioBytes, 0);
  const hasDictations = usable.some((x) => x.dictations > 0);

  const action = () => {
    if (run.phase === "running") {
      const pr = run.progress;
      const value = pr && pr.total > 0 ? pr.done / pr.total : 0;
      return (
        <SettingRow
          title="Importing…"
          description={pr?.title ?? "Reading the older databases."}
          control={
            <Progress
              value={value}
              width={160}
              align="end"
              caption={pr ? `${pr.done} of ${pr.total}` : "Starting"}
              label="Import progress"
            />
          }
        />
      );
    }
    if (run.phase === "error") {
      return (
        <SettingRow
          title="The import stopped."
          description={`${run.message} Meetings imported so far are kept.`}
          control={
            <Button variant="secondary" onClick={start}>
              Try again
            </Button>
          }
        />
      );
    }
    if (run.phase === "done") {
      const r = run.result;
      const shown = r.issues.slice(0, MAX_ISSUES);
      return (
        <>
          <SettingRow
            title={resultTitle(r)}
            description={resultLine(r)}
            control={
              remaining > 0 ? (
                <Button variant="secondary" onClick={start}>
                  Try again
                </Button>
              ) : undefined
            }
          />
          {shown.length > 0 ? (
            <ul className={s.issues} aria-label="Import notes">
              {shown.map((i) => (
                <li key={`${i.source}-${i.legacyId}-${i.reason}`} className={s.issue}>
                  <span className={s.issueKind}>{i.failed ? "Failed" : "Note"}</span>
                  <span className={s.issueText}>
                    <span className={s.issueTitle}>
                      {i.title ?? sources.find((x) => x.kind === i.source)?.name ?? "Older Minutae"}
                    </span>{" "}
                    {i.reason}
                  </span>
                </li>
              ))}
              {r.issues.length > MAX_ISSUES ? (
                <li className={s.issue}>
                  <span className={s.issueText}>and {r.issues.length - MAX_ISSUES} more. Details are in the log.</span>
                </li>
              ) : null}
            </ul>
          ) : null}
        </>
      );
    }
    if (usable.length === 0) return null;
    if (remaining === 0) {
      return <SettingRow title="Everything is imported" description="Meetings from the older app are in your list." />;
    }
    const what = hasDictations ? "meetings, transcripts, summaries and dictations" : "meetings, transcripts and summaries";
    const cost = audio > 0 ? ` Audio is copied and takes ${formatBytes(audio)} more ${localPhrase(platform)}.` : "";
    return (
      <SettingRow
        title="Import"
        description={`Copies ${what}. The older app's files stay as they are.${cost}`}
        control={
          <Button variant="secondary" onClick={start}>
            Import
          </Button>
        }
      />
    );
  };

  return (
    <section className={s.group} aria-labelledby="g-import" data-ready={String(scan.status !== "loading")}>
      <GroupLabel id="g-import">Import from older Minutae</GroupLabel>
      {scan.status === "loading" ? (
        <div className={p.loading}>
          <Spinner /> Looking for older Minutae data…
        </div>
      ) : !scan.data ? (
        <SettingRow
          title="Couldn't look for older Minutae data."
          description={scan.error ?? undefined}
          control={
            <Button variant="quiet" className={s.quietLink} onClick={() => void rescan()}>
              Try again
            </Button>
          }
        />
      ) : sources.length === 0 && run.phase !== "running" ? (
        <SettingRow
          title="Nothing to import"
          description={`No data from Minutae for Mac or Minutae 1 was found ${localPhrase(platform)}.`}
        />
      ) : (
        <>
          {sources.map((src) => (
            <SettingRow
              key={src.path}
              title={src.name}
              description={sourceLine(src)}
              control={
                <span className={s.path} title={src.path}>
                  {tildify(src.path)}
                </span>
              }
            />
          ))}
          {action()}
        </>
      )}
    </section>
  );
}
