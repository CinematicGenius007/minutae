import {
  Button,
  EmptyState,
  FilterChip,
  GroupLabel,
  Spinner,
  cx,
  formatClock,
  formatDuration,
  formatRelativeDate,
  localPhrase,
  usePlatform,
} from "@minutae/ui";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { useLocation, useSearch } from "wouter";
import { api } from "../../lib/api";
import type { Answer, SearchHit, SearchKind } from "../../lib/types";
import { errorText, FOCUS_SEARCH_EVENT, useDebounced, useLoad } from "../../hooks";
import p from "../settings/shared/page.module.css";
import s from "./library.module.css";

const FILTERS: { kind: SearchKind; label: string }[] = [
  { kind: "all", label: "All" },
  { kind: "meetings", label: "Meetings" },
  { kind: "dictations", label: "Dictations" },
  { kind: "actionItems", label: "Action items" },
];

const KIND_LABEL: Record<SearchHit["kind"], string> = {
  meeting: "Meeting",
  dictation: "Dictation",
  actionItem: "Action item",
  summary: "Summary",
};

type AskState =
  | { status: "idle" }
  | { status: "asking"; question: string }
  | { status: "done"; question: string; answer: Answer }
  | { status: "error"; question: string; message: string };

type SearchState =
  | { status: "idle" }
  | { status: "searching"; hits: SearchHit[] | null }
  | { status: "done"; hits: SearchHit[] }
  | { status: "error"; message: string };


/** Renders a snippet with its highlight ranges in ink semibold. */
function Highlighted({ text, ranges }: { text: string; ranges: [number, number][] }) {
  const parts: ReactNode[] = [];
  let at = 0;
  const sorted = [...ranges].filter(([a, b]) => b > a).sort((a, b) => a[0] - b[0]);
  for (const [a, b] of sorted) {
    const start = Math.max(a, at);
    const end = Math.min(b, text.length);
    if (start >= end) continue;
    if (start > at) parts.push(text.slice(at, start));
    parts.push(
      <mark key={start} className={s.mark}>
        {text.slice(start, end)}
      </mark>,
    );
    at = end;
  }
  if (at < text.length) parts.push(text.slice(at));
  return <>{parts}</>;
}

function hitWhen(h: SearchHit): string {
  if (h.atMs != null && h.kind !== "dictation") return formatDuration(h.atMs);
  const d = new Date(h.date);
  const rel = formatRelativeDate(d);
  return /^\d/.test(rel) && rel.includes(":") ? rel : `${rel} ${formatClock(d)}`;
}

function isOnDevice(provider: string): boolean {
  return /ollama|on[- ]device|local|on this|whisper/i.test(provider);
}

export default function LibraryScreen() {
  const platform = usePlatform();
  const [, navigate] = useLocation();
  // Route params live inside the hash (#/library?q=…&focus=1), never in location.search.
  const routeSearch = useSearch();
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<SearchKind>("all");
  const [search, setSearch] = useState<SearchState>({ status: "idle" });
  const [ask, setAsk] = useState<AskState>({ status: "idle" });
  const [retry, setRetry] = useState(0);
  const debounced = useDebounced(query.trim(), 180);

  // Header counts (best effort; hidden if either call fails).
  const [counts] = useLoad(async () => {
    const [groups, dictations] = await Promise.all([api.listMeetings(1000), api.listDictations(1000)]);
    return { meetings: groups.reduce((n, g) => n + g.items.length, 0), dictations: dictations.length };
  });

  // Autofocus when opened with ?focus=1 (⌘K from anywhere); ?q= prefills.
  useEffect(() => {
    const params = new URLSearchParams(routeSearch);
    const q = params.get("q");
    if (q) setQuery(q);
    if (params.get("focus") === "1") inputRef.current?.focus();
  }, [routeSearch]);
  useEffect(() => {
    const onFocus = (e: Event) => {
      const q = (e as CustomEvent<{ query?: string }>).detail?.query;
      if (q) setQuery(q);
      inputRef.current?.focus();
      inputRef.current?.select();
    };
    window.addEventListener(FOCUS_SEARCH_EVENT, onFocus);
    return () => window.removeEventListener(FOCUS_SEARCH_EVENT, onFocus);
  }, []);

  // Typing searches (debounced).
  useEffect(() => {
    if (!debounced) {
      setSearch({ status: "idle" });
      return;
    }
    let live = true;
    setSearch((prev) => ({ status: "searching", hits: prev.status === "done" ? prev.hits : null }));
    api
      .search(debounced, kind)
      .then((hits) => live && setSearch({ status: "done", hits }))
      .catch((e) => live && setSearch({ status: "error", message: errorText(e) }));
    return () => {
      live = false;
    };
  }, [debounced, kind, retry]);

  // Only the latest question may land: each ask (and each edit of the question) bumps the
  // sequence, and an answer for an older sequence is dropped.
  const askSeq = useRef(0);
  const askedFor = useRef<string | null>(null);
  const runAsk = (question: string) => {
    const q = question.trim();
    if (!q) return;
    const mine = ++askSeq.current;
    askedFor.current = q;
    setAsk({ status: "asking", question: q });
    api
      .ask(q)
      .then((answer) => mine === askSeq.current && setAsk({ status: "done", question: q, answer }))
      .catch((e) => mine === askSeq.current && setAsk({ status: "error", question: q, message: errorText(e) }));
  };

  // A new question (edited text) hides the stale answer and cancels one still in flight.
  useEffect(() => {
    if (askedFor.current === null || askedFor.current === query.trim()) return;
    askSeq.current++;
    askedFor.current = null;
    setAsk({ status: "idle" });
  }, [query]);

  const open = (h: SearchHit) => {
    if (h.meetingId) navigate(`/meetings/${h.meetingId}`);
    else if (h.dictationId) navigate("/dictate");
  };

  const hits = search.status === "done" ? search.hits : search.status === "searching" ? search.hits : null;
  const sources = ask.status === "done" && ask.answer.sources.length > 0 && !hits?.length ? ask.answer.sources : hits;
  const showAnswer = ask.status !== "idle";

  const overline = useMemo(() => {
    const local = `all ${localPhrase(platform)}`;
    if (!counts.data) return local;
    const m = counts.data.meetings;
    const d = counts.data.dictations;
    return `${m.toLocaleString()} ${m === 1 ? "meeting" : "meetings"} · ${d.toLocaleString()} ${d === 1 ? "dictation" : "dictations"} · ${local}`;
  }, [counts.data, platform]);

  return (
    <section
      className={cx(p.page, p.noToolbar)}
      aria-labelledby="library-title"
      data-screen="library"
      data-ready={String(counts.status !== "loading" && debounced === query.trim() && search.status !== "searching" && ask.status !== "asking")}
    >
      <div className={cx(p.inner, s.stack)}>
        <header className={s.header}>
          <div className={s.overline}>{overline}</div>
          <h1 id="library-title" className={s.title}>
            Library
          </h1>
        </header>

        <form
          className={s.field}
          role="search"
          onSubmit={(e) => {
            e.preventDefault();
            runAsk(query);
          }}
        >
          <svg className={s.fieldIcon} width="20" height="20" viewBox="0 0 24 24" aria-hidden>
            <circle cx="11" cy="11" r="7" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" />
            <path d="M20 20l-4-4" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" />
          </svg>
          <input
            ref={inputRef}
            className={s.input}
            type="search"
            value={query}
            onChange={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape" && query) {
                e.preventDefault();
                setQuery("");
              }
            }}
            placeholder="Search or ask anything you've said or heard"
            aria-label="Search or ask"
            aria-describedby="library-hint"
            autoComplete="off"
            spellCheck={false}
          />
          <span id="library-hint" className={s.hint}>
            ⏎ to ask
          </span>
        </form>

        <div className={s.filters} role="group" aria-label="Filter results">
          {FILTERS.map((f) => (
            <FilterChip key={f.kind} selected={kind === f.kind} onClick={() => setKind(f.kind)}>
              {f.label}
            </FilterChip>
          ))}
        </div>

        {!query.trim() && !showAnswer ? (
          <EmptyState
            title="Search every meeting and dictation."
            description="Type to find what was said. Press ⏎ to ask a question in plain words; the answer cites its sources."
          />
        ) : (
          <div className={cx(s.results, !showAnswer && s.resultsSingle)}>
            {showAnswer ? (
              <section className={s.answer} aria-live="polite" aria-busy={ask.status === "asking"}>
                <h2 className={p.label}>Answer</h2>
                {ask.status === "asking" ? (
                  <p className={s.asking}>
                    <Spinner /> Reading your library {localPhrase(platform)}…
                  </p>
                ) : ask.status === "error" ? (
                  <div className={s.answerError}>
                    <p className={s.errorText}>Couldn't answer that. {ask.message}</p>
                    <Button variant="quiet" onClick={() => runAsk(ask.question)}>
                      Try again
                    </Button>
                  </div>
                ) : ask.status === "done" ? (
                  <>
                    <p className={s.answerText}>{ask.answer.text}</p>
                    <p className={p.capsMeta}>
                      From {ask.answer.sources.length} {ask.answer.sources.length === 1 ? "source" : "sources"} ·{" "}
                      {isOnDevice(ask.answer.provider) ? "answered on-device" : `answered via ${ask.answer.provider}`}
                    </p>
                  </>
                ) : null}
              </section>
            ) : null}

            <section className={s.sources} aria-labelledby="sources-label" aria-busy={search.status === "searching"}>
              <GroupLabel id="sources-label" as="h2" count={sources ? sources.length : undefined}>
                Sources
              </GroupLabel>
              {search.status === "error" ? (
                <EmptyState
                  title="Search isn't working right now."
                  description={search.message}
                  action={
                    <Button variant="secondary" onClick={() => setRetry((n) => n + 1)}>
                      Try again
                    </Button>
                  }
                />
              ) : sources === null || sources === undefined ? (
                <div className={p.loading}>
                  <Spinner /> Searching…
                </div>
              ) : sources.length === 0 ? (
                <EmptyState
                  title={`Nothing matches “${query.trim()}”.`}
                  description={kind === "all" ? "Try fewer words, or press ⏎ to ask it as a question." : "Try All, or press ⏎ to ask it as a question."}
                />
              ) : (
                <ul>
                  {sources.map((h, i) => (
                    <li key={`${h.kind}-${h.meetingId ?? h.dictationId}-${i}`}>
                      <button type="button" className={s.hit} onClick={() => open(h)}>
                        <span className={s.hitHead}>
                          <span className={s.hitTitle}>{h.title}</span>
                          <span className={s.hitMeta}>
                            {KIND_LABEL[h.kind]} · {hitWhen(h)}
                          </span>
                        </span>
                        <span className={s.snippet}>
                          “<Highlighted text={h.snippet} ranges={h.highlights} />”
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          </div>
        )}
      </div>
    </section>
  );
}
