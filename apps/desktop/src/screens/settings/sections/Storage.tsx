import { Button, ConfirmDialog, GroupLabel, Segmented, SettingRow, Spinner, cx, formatBytes, localPhrase, usePlatform, useToast } from "@minutae/ui";
import { useState } from "react";
import { api } from "../../../lib/api";
import { tildify } from "../shared/format";
import { errorText, useLoad } from "../../../hooks";
import p from "../shared/page.module.css";
import s from "../settings.module.css";
import type { SectionProps } from "./types";

function revealLabel(platform: string): string {
  return platform === "mac" ? "Show in Finder" : platform === "windows" ? "Show in Explorer" : "Show in Files";
}

export function StorageSection({ settings, update }: SectionProps) {
  const platform = usePlatform();
  const toast = useToast();
  const [usage, reloadUsage] = useLoad(() => api.storageUsage());
  const [exporting, setExporting] = useState(false);
  const [erasing, setErasing] = useState(false);
  const [confirmErase, setConfirmErase] = useState(false);

  const u = usage.data;
  const total = u ? u.recordingsBytes + u.modelsBytes + u.indexBytes : 0;
  const pct = (n: number) => (total > 0 ? `${(n / total) * 100}%` : "0%");

  const pick = () => {
    api
      .pickFolder()
      .then((path) => {
        if (path) void update({ recordingsPath: path });
      })
      .catch((e) => toast.show({ title: `Couldn't open the folder picker. ${errorText(e)}` }));
  };

  const exportAll = () => {
    setExporting(true);
    api
      .exportAll()
      .then((path) => {
        if (!path) return;
        toast.show({ title: "Export ready", actionLabel: revealLabel(platform), onAction: () => void api.revealPath(path) });
      })
      .catch((e) => toast.show({ title: `Export failed. ${errorText(e)}` }))
      .finally(() => setExporting(false));
  };

  const erase = () => {
    setErasing(true);
    api
      .eraseAll()
      .then(() => {
        toast.show({ title: "All data erased" });
        void reloadUsage();
      })
      .catch((e) => toast.show({ title: `Couldn't erase everything. ${errorText(e)}` }))
      .finally(() => setErasing(false));
  };

  const recordingsFolder = settings.recordingsPath ?? (u ? `${u.dataDir}/recordings` : "");

  return (
    <>
      <section className={s.disk} aria-label="Disk usage" data-ready={String(usage.status !== "loading")}>
        {usage.status === "loading" ? (
          <div className={p.loading}>
            <Spinner /> Measuring…
          </div>
        ) : !u ? (
          <SettingRow
            title="Couldn't measure disk usage."
            description={usage.error ?? undefined}
            control={
              <Button variant="quiet" className={s.quietLink} onClick={() => void reloadUsage()}>
                Try again
              </Button>
            }
          />
        ) : (
          <>
            <div className={s.diskHead}>
              <div className={s.diskFigure}>
                <span className={s.diskTotal}>{formatBytes(total)}</span>
                <span className={s.diskCaption}>used {localPhrase(platform)}</span>
              </div>
              <Button variant="quiet" className={s.quietLink} onClick={() => void api.revealPath(u.dataDir)}>
                {revealLabel(platform)}
              </Button>
            </div>
            <div
              className={s.bar}
              role="img"
              aria-label={`Recordings ${formatBytes(u.recordingsBytes)}, models ${formatBytes(u.modelsBytes)}, notes and search index ${formatBytes(u.indexBytes)}`}
            >
              <span className={cx(s.seg, s.segRec)} style={{ width: pct(u.recordingsBytes) }} />
              <span className={cx(s.seg, s.segModels)} style={{ width: pct(u.modelsBytes) }} />
              <span className={cx(s.seg, s.segIndex)} style={{ width: pct(u.indexBytes) }} />
            </div>
            <ul className={s.legend} aria-hidden>
              <li className={s.legendItem}>
                <span className={cx(s.swatch, s.segRec)} /> Recordings {formatBytes(u.recordingsBytes)}
              </li>
              <li className={s.legendItem}>
                <span className={cx(s.swatch, s.segModels)} /> Models {formatBytes(u.modelsBytes)}
              </li>
              <li className={s.legendItem}>
                <span className={cx(s.swatch, s.segIndex)} /> Notes &amp; search index {formatBytes(u.indexBytes)}
              </li>
            </ul>
          </>
        )}
      </section>

      <section className={s.group} aria-labelledby="g-storage">
        <GroupLabel id="g-storage">Storage</GroupLabel>
        <SettingRow
          title="Delete audio after"
          description="Transcripts and summaries are always kept."
          control={
            <Segmented
              aria-label="Delete audio after"
              value={settings.deleteAudioAfterDays == null ? "never" : String(settings.deleteAudioAfterDays)}
              onValueChange={(v) => update({ deleteAudioAfterDays: v === "never" ? null : Number(v) })}
              options={[
                { value: "never", label: "Never" },
                { value: "30", label: "30 days" },
                { value: "90", label: "90 days" },
              ]}
            />
          }
        />
        <SettingRow
          title="Recordings folder"
          control={
            <>
              <span className={s.path} title={recordingsFolder}>
                {tildify(recordingsFolder)}
              </span>
              <Button variant="quiet" className={s.quietLink} onClick={pick} aria-label="Change recordings folder">
                Change
              </Button>
            </>
          }
        />
        <SettingRow
          title="Export everything"
          description="A zip of Markdown files, transcripts and audio."
          control={
            <Button variant="secondary" onClick={exportAll} loading={exporting}>
              Export…
            </Button>
          }
        />
        <SettingRow
          title={<span className={s.dangerTitle}>Erase all data</span>}
          description={`Removes every meeting, dictation and model from ${localPhrase(platform).replace(/^on /, "")}. Can't be undone.`}
          control={
            <Button variant="destructive" className={s.quietLink} onClick={() => setConfirmErase(true)} loading={erasing}>
              Erase…
            </Button>
          }
        />
      </section>

      <ConfirmDialog
        open={confirmErase}
        onOpenChange={setConfirmErase}
        title="Erase all data?"
        description={`Every meeting, recording, transcript, dictation and model is removed ${localPhrase(platform)}. This can't be undone. Export first if you want a copy.`}
        confirmLabel="Erase everything"
        onConfirm={erase}
      />
    </>
  );
}
