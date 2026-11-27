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
import { MoreHorizontal } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { api, events } from "../../lib/api";
import type { Dictation } from "../../lib/types";
import { toKitShortcut } from "../settings/shared/format";
import { errorText, useEvent, useLoad, usePlatformInfo, useSettings, useShortcut, useShortcutStatus } from "../../hooks";
import p from "../settings/shared/page.module.css";
import s from "./dictate.module.css";

const SPOKEN_KEY: Record<string, string> = {
  "⌘": "Command",
  "⌥": "Option",
  "⌃": "Control",
  "⇧": "Shift",
  "⏎": "Return",
  "↑": "Up",
  "↓": "Down",
  "←": "Left",
  "→": "Right",
  "⌫": "Backspace",
  "⌦": "Delete",
  esc: "Escape",
};

function spokenKeyLabel(label: string): string {
  return label
    .split(/\s+/)
    .map((part) => SPOKEN_KEY[part] ?? part)
    .join(" ");
}

function previewText(text: string): string {
  const trimmed = text.trim();
  return trimmed.length > 40 ? `${trimmed.slice(0, 40).trimEnd()}…` : trimmed;
}

export default function DictateScreen() {
  const platform = usePlatform();
  const session = usePlatformInfo()?.sessionKind;
  const copyOnly = session === "wayland";
  const toast = useToast();
  const { settings } = useSettings();
  const shortcuts = useShortcutStatus();
  const [list, reloadList] = useLoad(() => api.listDictations(100));
  const [stats, reloadStats] = useLoad(() => api.dictationStats());
  const [query, setQuery] = useState("");
  const [pendingDelete, setPendingDelete] = useState<Dictation | null>(null);

  const searchRef = useRef<HTMLInputElement>(null);
  const recentRef = useRef<HTMLElement>(null);
  const copyRefs = useRef<Map<string, HTMLButtonElement>>(new Map());
  const moreRefs = useRef<Map<string, HTMLButtonElement>>(new Map());
  const returnFocusId = useRef<string | null>(null);
  const focusAfterDelete = useRef<string | "search" | null>(null);
  const focusAfterRetry = useRef(false);

  useEvent<void>(events.dictationsChanged, () => {
    void reloadList();
    void reloadStats();
  });

  // "/" focuses this screen's filter field (matching Meetings); ⌘K / Ctrl+K stays global (Shell → Library).
  useShortcut("slash", () => searchRef.current?.focus());

  const hotkey = toKitShortcut(settings?.dictationHotkey ?? (platform === "mac" ? "Alt+Space" : "Ctrl+Space"));
  const keys = shortcutKeys(hotkey, platform);
  const keyLabel = shortcuts?.dictateTrigger ?? (platform === "mac" ? keys.join(" ") : keys.join(" + "));
  const spokenKey = spokenKeyLabel(keyLabel);
  const toggle = shortcuts?.holdSupported === false || settings?.dictationMode === "toggle";
  const hasShortcut = shortcuts?.backend !== "unavailable" && (shortcuts?.backend !== "portal" || shortcuts.dictateTrigger !== null);

  const rows = useMemo(() => {
    const all = list.data ?? [];
    const q = query.trim().toLowerCase();
    return q ? all.filter((d) => d.text.toLowerCase().includes(q) || d.targetApp?.toLowerCase().includes(q)) : all;
  }, [list.data, query]);

  // After a confirmed delete removes a row, move focus to the adjacent row's Copy button (or search when empty).
  useEffect(() => {
    const target = focusAfterDelete.current;
    if (!target || list.status !== "ready") return;
    if (target === "search") {
      focusAfterDelete.current = null;
      searchRef.current?.focus();
      return;
    }
    const nextEl = copyRefs.current.get(target);
    if (nextEl) {
      focusAfterDelete.current = null;
      nextEl.focus();
    } else if (rows.length === 0) {
      focusAfterDelete.current = null;
      searchRef.current?.focus();
    }
  }, [list.status, rows]);

  // After a keyboard/AT retry recovers from an error, move focus from the unmounted "Try again"
  // button to the Recent heading so screen readers land at the top of the loaded section without
  // leaving a stray focus ring on a mouse retry.
  useEffect(() => {
    if (!focusAfterRetry.current || list.status === "loading") return;
    focusAfterRetry.current = false;
    if (list.status !== "ready") return;
    if (document.activeElement && document.activeElement !== document.body) return;
    recentRef.current?.focus();
  }, [list]);

  const copy = (d: Dictation) => {
    api
      .copyText(d.text)
      .then(() => toast.show({ title: "Dictation copied" }))
      .catch((e) => toast.show({ title: `Couldn't copy. ${errorText(e)}` }));
  };

  const remove = (d: Dictation) => {
    const idx = rows.findIndex((r) => r.id === d.id);
    const nextRow = rows[idx + 1] ?? rows[idx - 1] ?? null;
    focusAfterDelete.current = nextRow ? nextRow.id : "search";
    api
      .deleteDictation(d.id)
      .then(() => {
        toast.show({ title: "Dictation deleted" });
        void reloadList();
        void reloadStats();
      })
      .catch((e) => {
        focusAfterDelete.current = null;
        toast.show({ title: `Couldn't delete it. ${errorText(e)}` });
      });
  };

  const clearSearch = () => {
    setQuery("");
    searchRef.current?.focus();
  };

  const retentionDays = settings?.dictationRetentionDays;
  const historyLimit = retentionDays != null ? `Keep for ${retentionDays} days` : undefined;
  const trimmedQuery = query.trim();
  const filterAnnouncement =
    trimmedQuery && list.status === "ready"
      ? rows.length === 0
        ? `No dictations match “${trimmedQuery}”.`
        : rows.length === 1
          ? `1 dictation matches “${trimmedQuery}”.`
          : `${rows.length} dictations match “${trimmedQuery}”.`
      : "";

  const shortcutSpan =
    spokenKey !== keyLabel ? (
      <>
        <span aria-hidden>{keyLabel}</span>
        <span className="mn-sr-only">{spokenKey}</span>
      </>
    ) : (
      keyLabel
    );

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
            ref={searchRef}
            className={cx(p.toolbarSearch, s.search)}
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
                <div className={s.keyGroup} aria-hidden>
                  <KeyCap className={s.bigCap}>{keyLabel}</KeyCap>
                  <span className={s.keyHint}>{toggle ? "press" : "hold"}</span>
                </div>
              )}
              <div className={s.howtoText}>
                <p className={s.howtoTitle}>
                  {!hasShortcut ? (
                    "Set up a system shortcut to dictate."
                  ) : toggle ? (
                    <>Press {shortcutSpan}, talk, press again.</>
                  ) : (
                    <>Hold {shortcutSpan}, talk, let go.</>
                  )}
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

          <section aria-labelledby="recent-label" aria-busy={list.status === "loading" || undefined}>
            <GroupLabel ref={recentRef} as="h2" id="recent-label" tabIndex={-1} count={historyLimit}>
              Recent
            </GroupLabel>
            <div className="mn-sr-only" role="status" aria-live="polite" aria-atomic="true">
              {filterAnnouncement}
            </div>
            {list.status === "loading" ? (
              <div className={p.loading} role="status" aria-live="polite">
                <Spinner /> Loading dictations…
              </div>
            ) : list.status === "error" && !list.data ? (
              <div role="alert">
                <EmptyState
                  title="Couldn't load your dictations."
                  description={list.error}
                  action={
                    <Button
                      variant="secondary"
                      onClick={(e) => {
                        focusAfterRetry.current = e.detail === 0;
                        void reloadList();
                        void reloadStats();
                      }}
                    >
                      Try again
                    </Button>
                  }
                />
              </div>
            ) : (list.data ?? []).length === 0 ? (
              <EmptyState
                title="No dictations yet."
                description={hasShortcut ? "Completed dictations will appear here." : "Set up a shortcut in Settings → Dictation to get started."}
              />
            ) : rows.length === 0 ? (
              <EmptyState
                title={`Nothing matches “${trimmedQuery}”.`}
                description="Try fewer words, or search everything in Library."
                action={
                  <Button variant="quiet" onClick={clearSearch}>
                    Clear search
                  </Button>
                }
              />
            ) : (
              // biome-ignore lint/a11y/noRedundantRoles: list-style:none strips implicit list semantics in WebKit/VoiceOver
              <ul className={s.list} role="list">
                {rows.map((d) => {
                  const snippet = previewText(d.text);
                  return (
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
                        <Button
                          ref={(el) => {
                            if (el) copyRefs.current.set(d.id, el);
                            else copyRefs.current.delete(d.id);
                          }}
                          variant="quiet"
                          className={s.copy}
                          onClick={() => copy(d)}
                          aria-label={`Copy “${snippet}”`}
                        >
                          Copy
                        </Button>
                        <DropdownMenu
                          align="end"
                          platform={platform}
                          trigger={
                            <button
                              ref={(el) => {
                                if (el) moreRefs.current.set(d.id, el);
                                else moreRefs.current.delete(d.id);
                              }}
                              type="button"
                              className={s.more}
                              aria-label={`More actions for “${snippet}”`}
                            >
                              <MoreHorizontal size={14} strokeWidth={2} aria-hidden />
                            </button>
                          }
                          items={[
                            { label: "Copy", onSelect: () => copy(d) },
                            "separator",
                            {
                              label: "Delete…",
                              destructive: true,
                              onSelect: () => {
                                returnFocusId.current = d.id;
                                setPendingDelete(d);
                              },
                            },
                          ]}
                        />
                      </span>
                    </li>
                  );
                })}
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
        onCloseAutoFocus={(e) => {
          e.preventDefault();
          const targetId = returnFocusId.current;
          returnFocusId.current = null;
          if (focusAfterDelete.current) {
            const nextTarget = focusAfterDelete.current;
            if (nextTarget === "search") {
              focusAfterDelete.current = null;
              searchRef.current?.focus();
            } else {
              const nextEl = copyRefs.current.get(nextTarget);
              if (nextEl) {
                focusAfterDelete.current = null;
                nextEl.focus();
              }
            }
            return;
          }
          if (targetId) moreRefs.current.get(targetId)?.focus();
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
