import { Button, EmptyState, Spinner, cx, useToast } from "@minutae/ui";
import { useCallback } from "react";
import { Link, useParams } from "wouter";
import type { AppSettings, SettingsSection } from "../../lib/types";
import { DictationSection } from "./sections/Dictation";
import { GeneralSection } from "./sections/General";
import { StorageSection } from "./sections/Storage";
import { SummariesSection } from "./sections/Summaries";
import { TranscriptionSection } from "./sections/Transcription";
import { UsageSection } from "./sections/Usage";
import { errorText, useSettings } from "../../hooks";
import p from "./shared/page.module.css";
import s from "./settings.module.css";

const SECTIONS: { id: SettingsSection; label: string }[] = [
  { id: "general", label: "General" },
  { id: "transcription", label: "Transcription" },
  { id: "summaries", label: "Summaries" },
  { id: "dictation", label: "Dictation" },
  { id: "storage", label: "Storage" },
  { id: "usage", label: "Usage" },
];

function isSection(v: string | undefined): v is SettingsSection {
  return SECTIONS.some((x) => x.id === v);
}

export default function SettingsScreen() {
  const params = useParams<{ section?: string }>();
  const section: SettingsSection = isSection(params.section) ? params.section : "general";
  const { settings, error, reload, update: write } = useSettings();
  const toast = useToast();
  // Every control applies immediately; on failure the store rolls back and a toast says so.
  const update = useCallback(
    (patch: Partial<AppSettings>) =>
      write(patch).then(
        () => undefined,
        (e) => {
          toast.show({ title: `Couldn't save that setting. ${errorText(e)}` });
        },
      ),
    [write, toast],
  );
  const ctx = { settings, error, reload, update };
  const label = SECTIONS.find((x) => x.id === section)?.label ?? "General";

  // Usage and Storage read their own data; the rest need settings first.
  const needsSettings = section !== "usage";

  return (
    <section className={cx(p.page, p.noToolbar)} aria-labelledby="settings-title">
      <div className={cx(p.inner, s.layout)}>
        <nav className={s.index} aria-label="Settings sections">
          <h1 id="settings-title" className={s.title}>
            Settings
          </h1>
          <ul className={s.links}>
            {SECTIONS.map((x) => (
              <li key={x.id}>
                <Link
                  href={`/settings/${x.id}`}
                  className={cx(s.link, x.id === section && s.linkActive)}
                  aria-current={x.id === section ? "page" : undefined}
                >
                  <span data-text={x.label}>{x.label}</span>
                </Link>
              </li>
            ))}
          </ul>
        </nav>

        <div className={s.groups} aria-label={`${label} settings`} role="region">
          {needsSettings && !ctx.settings ? (
            ctx.error ? (
              <EmptyState
                title="Couldn't load your settings."
                description={ctx.error}
                action={
                  <Button variant="secondary" onClick={ctx.reload}>
                    Try again
                  </Button>
                }
              />
            ) : (
              <div className={p.loading}>
                <Spinner /> Loading settings…
              </div>
            )
          ) : section === "general" && ctx.settings ? (
            <GeneralSection settings={ctx.settings} update={ctx.update} />
          ) : section === "transcription" && ctx.settings ? (
            <TranscriptionSection settings={ctx.settings} update={ctx.update} />
          ) : section === "summaries" && ctx.settings ? (
            <SummariesSection settings={ctx.settings} update={ctx.update} />
          ) : section === "dictation" && ctx.settings ? (
            <DictationSection settings={ctx.settings} update={ctx.update} />
          ) : section === "storage" && ctx.settings ? (
            <StorageSection settings={ctx.settings} update={ctx.update} />
          ) : (
            <UsageSection />
          )}
        </div>
      </div>
    </section>
  );
}
