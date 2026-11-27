import {
  Button,
  ConfirmDialog,
  GroupLabel,
  Progress,
  RadioGroup,
  RadioItem,
  Segmented,
  Select,
  SettingRow,
  Spinner,
  Toggle,
  formatBytes,
  localPhrase,
  usePlatform,
  useToast,
} from "@minutae/ui";
import { useCallback, useEffect, useState } from "react";
import { api, events } from "../../../lib/api";
import type { DiarizeModelInfo, ModelInfo, WhisperModel } from "../../../lib/types";
import { localTitle, tildify } from "../shared/format";
import { errorText, useLoad } from "../../../hooks";
import p from "../shared/page.module.css";
import s from "../settings.module.css";
import type { SectionProps } from "./types";

const MODEL_COPY: Record<WhisperModel, { name: string; desc: string }> = {
  tiny: { name: "Tiny", desc: "Fastest. Fine for short, clear calls." },
  base: { name: "Base", desc: "Good balance for most laptops." },
  small: { name: "Small", desc: "Recommended. Handles accents and crosstalk well." },
  medium: { name: "Medium", desc: "Most accurate. Needs 16 GB memory." },
};

const LANGUAGES = [
  { value: "auto", label: "Detect automatically" },
  "separator" as const,
  { value: "en", label: "English" },
  { value: "es", label: "Spanish" },
  { value: "fr", label: "French" },
  { value: "de", label: "German" },
  { value: "it", label: "Italian" },
  { value: "pt", label: "Portuguese" },
  { value: "nl", label: "Dutch" },
  { value: "hi", label: "Hindi" },
  { value: "ja", label: "Japanese" },
  { value: "ko", label: "Korean" },
  { value: "zh", label: "Chinese" },
];

export function TranscriptionSection({ settings, update }: SectionProps) {
  const platform = usePlatform();
  const toast = useToast();
  const [models, setModels] = useState<ModelInfo[] | null>(null);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [diarize, setDiarize] = useState<DiarizeModelInfo | null>(null);
  const [diarizeChecked, setDiarizeChecked] = useState(false);
  const [storage] = useLoad(() => api.storageUsage());
  const [pendingDelete, setPendingDelete] = useState<WhisperModel | null>(null);

  const refresh = useCallback(() => {
    api
      .listModels()
      .then((m) => {
        setModels(m);
        setModelsError(null);
      })
      .catch((e) => setModelsError(errorText(e)));
    api
      .diarizeModel()
      .then(setDiarize)
      .catch(() => setDiarize(null))
      .finally(() => setDiarizeChecked(true));
  }, []);

  useEffect(() => {
    refresh();
    let off: (() => void) | null = null;
    let dead = false;
    void events
      .modelProgress((ev) => {
        if (ev.error) {
          toast.show({ title: `Download stopped. ${ev.error}` });
          refresh();
          return;
        }
        if (ev.model === "diarize") {
          setDiarize((d) => (d ? { ...d, state: ev.done ? "ready" : "downloading", fraction: ev.done ? null : ev.fraction } : d));
        } else {
          setModels((ms) =>
            ms?.map((m) =>
              m.model === ev.model ? { ...m, state: ev.done ? "ready" : "downloading", fraction: ev.done ? null : ev.fraction } : m,
            ) ?? ms,
          );
        }
        if (ev.done) refresh();
      })
      .then((u) => {
        if (dead) u();
        else off = u;
      });
    return () => {
      dead = true;
      off?.();
    };
  }, [refresh, toast]);

  // Events can be missed (screen opened mid-download, listener still attaching), so while
  // anything downloads, also ask the backend every 2 s. Stops as soon as nothing is downloading.
  const anyDownloading = models?.some((m) => m.state === "downloading") || diarize?.state === "downloading";
  useEffect(() => {
    if (!anyDownloading) return;
    const t = window.setInterval(refresh, 2000);
    return () => window.clearInterval(t);
  }, [anyDownloading, refresh]);

  const download = (model: WhisperModel) => {
    setModels((ms) => ms?.map((m) => (m.model === model ? { ...m, state: "downloading", fraction: 0 } : m)) ?? ms);
    api.downloadModel(model).catch((e) => {
      toast.show({ title: `Couldn't start the download. ${errorText(e)}` });
      refresh();
    });
  };
  const cancel = (model: WhisperModel) => {
    api
      .cancelModelDownload(model)
      .then(refresh)
      .catch((e) => toast.show({ title: `Couldn't cancel. ${errorText(e)}` }));
  };
  const remove = (model: WhisperModel) => {
    api
      .deleteModel(model)
      .then(() => {
        toast.show({ title: `${MODEL_COPY[model].name} model deleted` });
        refresh();
      })
      .catch((e) => toast.show({ title: `Couldn't delete the model. ${errorText(e)}` }));
  };
  const choose = (model: WhisperModel) => {
    void update({ whisperModel: model });
    const info = models?.find((m) => m.model === model);
    if (info?.state === "missing") download(model);
  };
  const downloadDiarize = () => {
    setDiarize((d) => (d ? { ...d, state: "downloading", fraction: 0 } : d));
    api.downloadDiarizeModel().catch((e) => {
      toast.show({ title: `Couldn't start the download. ${errorText(e)}` });
      refresh();
    });
  };

  const local = localTitle(platform);
  const modelsDir = storage.data ? `${tildify(storage.data.dataDir)}/models` : undefined;

  return (
    <>
      <section
        className={s.group}
        aria-labelledby="g-transcription"
        data-ready={String((models !== null || modelsError !== null) && diarizeChecked && storage.status !== "loading")}
      >
        <GroupLabel id="g-transcription">Transcription</GroupLabel>
        <SettingRow
          title="Where it runs"
          description={`Runs ${localPhrase(platform)}, free, private and offline. Cloud transcription is planned for a future release.`}
          control={
            <Segmented
              aria-label="Where transcription runs"
              value="local"
              onValueChange={(v) => {
                if (v === "local") void update({ transcribeWhere: v });
              }}
              options={[
                { value: "local", label: local },
                {
                  value: "cloud",
                  label: "Cloud",
                  tag: "Planned",
                  disabled: true,
                  disabledReason: "Cloud transcription is planned for a future release.",
                },
              ]}
            />
          }
        />
        <SettingRow
          title="Language"
          htmlFor="s-language"
          description="Pick one if calls are always in the same language. It's faster."
          control={
            <Select
              id="s-language"
              options={LANGUAGES}
              value={settings.language ?? "auto"}
              onValueChange={(v) => update({ language: v === "auto" ? null : v })}
            />
          }
        />
        <SettingRow
          title="Label speakers"
          htmlFor="s-speakers"
          description={
            diarize?.state === "missing"
              ? `Splits the transcript by voice. Needs a ${formatBytes(diarize.sizeBytes)} speaker model.`
              : "Splits the transcript by voice. Adds about 20% to processing time."
          }
          control={
            <>
              {diarize?.state === "missing" && settings.labelSpeakers ? (
                <Button variant="quiet" className={s.quietLink} onClick={downloadDiarize}>
                  Download model
                </Button>
              ) : null}
              {diarize?.state === "downloading" ? (
                <Progress
                  value={diarize.fraction ?? 0}
                  width={96}
                  align="end"
                  caption={`${Math.round((diarize.fraction ?? 0) * 100)}%`}
                  label="Speaker model download"
                />
              ) : null}
              <Toggle id="s-speakers" checked={settings.labelSpeakers} onCheckedChange={(v) => update({ labelSpeakers: v })} />
            </>
          }
        />
        <SettingRow
          title="Live captions"
          htmlFor="s-captions"
          description={
            models && !models.some((m) => m.state === "ready")
              ? "Shows what's said while you record. Needs a downloaded model below."
              : "Shows what's said while you record, using a small model in the background. Uses some battery."
          }
          control={
            <Toggle id="s-captions" checked={settings.liveCaptions} onCheckedChange={(v) => update({ liveCaptions: v })} />
          }
        />
        <SettingRow
          title="Echo cancellation"
          description="Removes speaker sound picked up by your mic, so no one is transcribed twice."
          control={
            <Segmented
              aria-label="Echo cancellation"
              value={settings.echoCancellation}
              onValueChange={(v) => update({ echoCancellation: v })}
              options={[
                { value: "auto", label: "Auto" },
                { value: "off", label: "Off" },
              ]}
            />
          }
        />
      </section>

      <section className={s.group} aria-labelledby="g-model">
        <GroupLabel id="g-model" count={modelsDir}>
          Model
        </GroupLabel>
        {modelsError && !models ? (
          <SettingRow
            title="Couldn't list the models."
            description={modelsError}
            control={
              <Button variant="quiet" className={s.quietLink} onClick={refresh}>
                Try again
              </Button>
            }
          />
        ) : !models ? (
          <div className={p.loading}>
            <Spinner /> Checking models…
          </div>
        ) : (
          <RadioGroup
            aria-label="Whisper model"
            value={settings.whisperModel}
            onValueChange={(v) => choose(v as WhisperModel)}
            style={{ gap: 0 }}
          >
            {models.map((m) => {
              const copy = MODEL_COPY[m.model];
              const id = `model-${m.model}`;
              const pct = Math.round((m.fraction ?? 0) * 100);
              return (
                <div key={m.model} className={s.modelRow}>
                  <RadioItem id={id} value={m.model} aria-describedby={`${id}-desc`} />
                  <label htmlFor={id} className={s.modelName}>
                    {copy.name}
                  </label>
                  <span id={`${id}-desc`} className={s.modelDesc}>
                    {copy.desc}
                  </span>
                  <span className={s.modelSize}>{formatBytes(m.sizeBytes)}</span>
                  <span className={s.modelState}>
                    {m.state === "missing" ? (
                      <Button variant="quiet" className={s.quietLink} onClick={() => download(m.model)} aria-label={`Download ${copy.name} model`}>
                        Download
                      </Button>
                    ) : m.state === "downloading" ? (
                      <>
                        <Progress value={m.fraction ?? 0} width={96} label={`${copy.name} model download, ${pct}%`} />
                        <span className={s.progressCaption}>
                          {pct >= 100 ? (
                            "Checking…"
                          ) : (
                            <>
                              {pct}% ·{" "}
                              <button type="button" onClick={() => cancel(m.model)} aria-label={`Cancel ${copy.name} model download`}>
                                Cancel
                              </button>
                            </>
                          )}
                        </span>
                      </>
                    ) : (
                      <>
                        <span className={`${s.installed} ${m.model !== settings.whisperModel ? s.installedHides : ""}`}>
                          Installed
                        </span>
                        {m.model !== settings.whisperModel ? (
                          <span className={s.reveal}>
                            <Button
                              variant="destructive"
                              className={s.quietLink}
                              onClick={() => setPendingDelete(m.model)}
                              aria-label={`Delete ${copy.name} model`}
                            >
                              Delete
                            </Button>
                          </span>
                        ) : null}
                      </>
                    )}
                  </span>
                </div>
              );
            })}
          </RadioGroup>
        )}
      </section>

      <ConfirmDialog
        open={pendingDelete !== null}
        onOpenChange={(o) => {
          if (!o) setPendingDelete(null);
        }}
        title={pendingDelete ? `Delete the ${MODEL_COPY[pendingDelete].name} model?` : "Delete model?"}
        description="You can download it again any time."
        confirmLabel="Delete model"
        onConfirm={() => {
          if (pendingDelete) remove(pendingDelete);
          setPendingDelete(null);
        }}
      />
    </>
  );
}
