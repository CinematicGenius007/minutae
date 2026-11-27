import {
  Button,
  ConfirmDialog,
  DropdownMenu,
  EmptyState,
  GroupLabel,
  KeyCap,
  PageHeader,
  SearchField,
  Spinner,
  Stat,
  cx,
  formatRelativeDate,
  shortcutKeys,
  usePlatform,
  useToast,
} from "@minutae/ui";
import { useMemo, useState } from "react";
import { api, events } from "../../lib/api";
import type { Dictation } from "../../lib/types";
import { toKitShortcut } from "../settings/shared/format";
import { errorText, useEvent, useLoad, usePlatformInfo, useSettings, useShortcutStatus } from "../../hooks";
import p from "../settings/shared/page.module.css";
import s from "./dictate.module.css";

export default function DictateScreen() {
  const platform = usePlatform();
  const session = usePlatformInfo()?.sessionKind;
  const copyOnly = session === "wayland" || session === "x11";
  const toast = useToast();
  const { settings } = useSettings();
  const shortcuts = useShortcutStatus();
  const [list, reloadList] = useLoad(() => api.listDictations(100));
  const [stats, reloadStats] = useLoad(() => api.dictationStats());
  const [query, setQuery] = useState("");
  const [pendingDelete, setPendingDelete] = useState<Dictation | null>(null);

  useEvent<void>(events.dictationsChanged, () => {
    void reloadList();
    void reloadStats();
  });

  // ⌘K / Ctrl+K stays global (Shell → Library), as on every other screen. This field only
  // filters the list below, so it shows no shortcut hint.

  const hotkey = toKitShortcut(settings?.dictationHotkey ?? (platform === "mac" ? "Alt+Space" : "Ctrl+Space"));
  const keys = shortcutKeys(hotkey, platform);
  const keyLabel = shortcuts?.dictateTrigger ?? (platform === "mac" ? keys.join(" ") : keys.join(" + "));
  const toggle = shortcuts?.holdSupported === false || settings?.dictationMode === "toggle";
  const hasShortcut = shortcuts?.backend !== "unavailable" && (shortcuts?.backend !== "portal" || shortcuts.dictateTrigger !== null);

  const rows = useMemo(() => {
    const all = list.data ?? [];
    const q = query.trim().toLowerCase();
    return q ? all.filter((d) => d.text.toLowerCase().includes(q) || d.targetApp?.toLowerCase().includes(q)) : all;
  }, [list.data, query]);

  const copy = (d: Dictation) => {
    api
      .copyText(d.text)
      .then(() => toast.show({ title: "Dictation copied" }))
      .catch((e) => toast.show({ title: `Couldn't copy. ${errorText(e)}` }));
  };

  const remove = (d: Dictation) => {
    api
      .deleteDictation(d.id)
      .then(() => {
        toast.show({ title: "Dictation deleted" });
        void reloadList();
        void reloadStats();
      })
      .catch((e) => toast.show({ title: `Couldn't delete it. ${errorText(e)}` }));
  };

  const retentionDays = settings?.dictationRetentionDays;
  const historyLimit = retentionDays != null ? `Keep for ${retentionDays} days` : undefined;

  return (
    <section
      className={p.page}
      aria-labelledby="dictate-title"
      data-screen="dictate"
      data-ready={String(list.status !== "loading" && stats.status !== "loading" && settings !== null && shortcuts !== null)}
    >
      <div className={cx(p.inner, s.stack)}>
        <div className={p.toolbar}>
          <SearchField
            className={p.toolbarSearch}
            placeholder="Search dictations"
            value={query}
            onChange={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setQuery("");
            }}
            platform={platform}
            shortcut={null}
          />
        </div>

        <PageHeader
          overline={copyOnly ? "Speak anywhere · Copies to your clipboard" : "Speak anywhere · Types where your cursor is"}
          title={<span id="dictate-title">Dictate</span>}
          right={
            stats.data ? (
              <>
                <Stat value={stats.data.wordsThisWeek.toLocaleString()} caption="Words this week" />
                <Stat value={`${Math.round(stats.data.minutesSaved)} min`} caption="Typing saved" />
              </>
            ) : null
          }
        />

        <div className={s.body}>
          <section className={s.howto} aria-label="How to dictate">
            <div className={s.howtoLead}>
              {hasShortcut && (
                <div className={s.keyGroup}>
                  <KeyCap className={s.bigCap}>{keyLabel}</KeyCap>
                  <span className={s.keyHint}>{toggle ? "press" : "hold"}</span>
                </div>
              )}
              <div className={s.howtoText}>
                <p className={s.howtoTitle}>
                  {!hasShortcut
                    ? "Set up a system shortcut to dictate."
                    : toggle ? `Press ${keyLabel}, talk, press again.` : `Hold ${keyLabel}, talk, let go.`}
                </p>
                <p className={s.howtoBody}>
                  {hasShortcut
                    ? copyOnly ? "Paste the copied text into your app. " : "Text lands in whatever app you're in. "
                    : "See Settings → Dictation. "}
                  {hasShortcut && !toggle && "Double-tap to keep listening hands-free. "}
                  {shortcuts?.cancelDetail ?? "Press Esc to cancel."}
                </p>
              </div>
            </div>
          </section>

          <section aria-labelledby="recent-label">
            <GroupLabel id="recent-label" count={historyLimit}>
              Recent
            </GroupLabel>
            {list.status === "loading" ? (
              <div className={p.loading}>
                <Spinner /> Loading dictations…
              </div>
            ) : list.status === "error" && !list.data ? (
              <EmptyState
                title="Couldn't load your dictations."
                description={list.error}
                action={
                  <Button variant="secondary" onClick={() => void reloadList()}>
                    Try again
                  </Button>
                }
              />
            ) : (list.data ?? []).length === 0 ? (
              <EmptyState
                title="No dictations yet."
                description={hasShortcut ? "Completed dictations will appear here." : "Set up a shortcut in Settings → Dictation to get started."}
              />
            ) : rows.length === 0 ? (
              <EmptyState
                title={`Nothing matches “${query.trim()}”.`}
                description="Try fewer words, or search everything in Library."
                action={
                  <Button variant="quiet" onClick={() => setQuery("")}>
                    Clear search
                  </Button>
                }
              />
            ) : (
              <ul className={s.list}>
                {rows.map((d) => (
                  <li key={d.id} className={s.row}>
                    <time className={s.time} dateTime={d.createdAt}>
                      {formatRelativeDate(new Date(d.createdAt))}
                    </time>
                    <p className={s.text}>{d.text}</p>
                    <span className={s.app}>{d.targetApp ?? "Copied"}</span>
                    <span className={s.words}>
                      {d.wordCount} {d.wordCount === 1 ? "word" : "words"}
                    </span>
                    <span className={s.actions}>
                      <button type="button" className={s.copy} onClick={() => copy(d)} aria-label={`Copy “${d.text.slice(0, 40)}”`}>
                        Copy
                      </button>
                      <DropdownMenu
                        align="end"
                        platform={platform}
                        trigger={
                          <button type="button" className={s.more} aria-label="More actions">
                            <svg width="14" height="14" viewBox="0 0 24 24" aria-hidden>
                              <circle cx="5" cy="12" r="1.8" fill="currentColor" />
                              <circle cx="12" cy="12" r="1.8" fill="currentColor" />
                              <circle cx="19" cy="12" r="1.8" fill="currentColor" />
                            </svg>
                          </button>
                        }
                        items={[
                          { label: "Copy", onSelect: () => copy(d) },
                          "separator",
                          { label: "Delete…", destructive: true, onSelect: () => setPendingDelete(d) },
                        ]}
                      />
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      </div>

      <ConfirmDialog
        open={pendingDelete !== null}
        onOpenChange={(o) => {
          if (!o) setPendingDelete(null);
        }}
        title="Delete this dictation?"
        description="The text and any saved audio are removed from Minutae. Anything already typed into other apps stays."
        confirmLabel="Delete dictation"
        onConfirm={() => {
          if (pendingDelete) remove(pendingDelete);
          setPendingDelete(null);
        }}
      />
    </section>
  );
}
