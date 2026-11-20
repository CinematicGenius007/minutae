// 01 Meetings (Paper 9P-0): date overline, "Meetings", week stats; day groups of lane rows.
// Refetches on meetings://changed; patches row progress in place on job://progress.
import { useMemo, useRef, useState } from "react";
import {
  Button,
  EmptyState,
  formatDuration,
  formatRelativeDate,
  formatShortcut,
  GroupLabel,
  LaneRow,
  localPhrase,
  PageHeader,
  Pill,
  Progress,
  SearchField,
  Stat,
  StatusDot,
  useToast,
} from "@minutae/ui";
import { openSearch, toError, useEvent, useFailureFix, useGo, useNarrow, usePlatform, useQuery, useShortcut } from "../../hooks";
import { api, events } from "../../lib/api";
import type { DayGroup, MeetingListItem, Progress as JobProgress } from "../../lib/types";
import s from "./Meetings.module.css";

const DAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
/** "Thursday, 20 November" (§9: day before month). */
const longDate = (d: Date) => `${DAYS[d.getDay()]}, ${d.getDate()} ${MONTHS[d.getMonth()]}`;

export function formatHoursMinutes(ms: number): string {
  const m = Math.round(ms / 60_000);
  if (m < 60) return `${m}m`;
  return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`;
}

function people(list: string[]): string {
  return list.join(", ");
}

function matches(item: MeetingListItem, q: string): boolean {
  const hay = `${item.title} ${item.takeaway ?? ""} ${item.people.join(" ")}`.toLowerCase();
  return q
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((w) => hay.includes(w));
}

export default function MeetingsScreen() {
  const go = useGo();
  const platform = usePlatform();
  const narrow = useNarrow();
  const toast = useToast();
  const [query, setQuery] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const fixFailure = useFailureFix();

  const list = useQuery(() => api.listMeetings(), [], { refetchOn: [events.meetingsChanged] });
  const stats = useQuery(() => api.weekStats(), [], { refetchOn: [events.meetingsChanged] });

  // Live progress: patch the row; refetch once a job finishes.
  useEvent(events.progress, (p: JobProgress) => {
    if (!p.meetingId) return;
    if (p.fraction >= 1) {
      void list.refetch();
      return;
    }
    list.setData((prev) =>
      prev?.map((g) => ({
        ...g,
        items: g.items.map((it) => (it.id === p.meetingId ? { ...it, status: "processing", progress: p } : it)),
      })),
    );
  });

  // Search field: "/" focuses it here; ⌘K (shell) opens Library.
  useShortcut("slash", () => searchRef.current?.focus());

  const groups: DayGroup[] | undefined = useMemo(() => {
    if (!list.data || !query.trim()) return list.data;
    return list.data
      .map((g) => ({ ...g, items: g.items.filter((it) => matches(it, query)) }))
      .filter((g) => g.items.length > 0);
  }, [list.data, query]);

  const retry = async (id: string) => {
    try {
      await api.retryMeeting(id);
      toast.show({ title: "Trying again" });
      void list.refetch();
    } catch (e) {
      toast.show({ title: `Couldn't retry. ${toError(e).message}` });
    }
  };

  const status = (it: MeetingListItem) => {
    switch (it.status) {
      case "recording":
        return <StatusDot tone="onair" label="On air" />;
      case "processing": {
        const pct = Math.round((it.progress?.fraction ?? 0) * 100);
        return (
          <Progress
            className={s.progress}
            width="calc(var(--s-12) + var(--s-7))"
            value={it.progress?.fraction ?? 0}
            caption={`${pct}%`}
            label={`${it.takeaway ?? "Processing"} ${pct}%`}
            align="end"
          />
        );
      }
      case "failed": {
        const fix = it.failure?.fix;
        return (
          <span className={s.failed}>
            <span className={s.failedLabel}>Not summarized</span>
            <Button
              variant="quiet"
              className={s.fix}
              onClick={(e) => {
                e.stopPropagation();
                if (fix) fixFailure(it.failure?.kind, () => void retry(it.id));
                else void retry(it.id);
              }}
              onKeyDown={(e) => e.stopPropagation()}
            >
              {fix ?? "Retry"}
            </Button>
          </span>
        );
      }
      default:
        return it.openActions > 0 ? (
          <Pill>{it.openActions === 1 ? "1 action" : `${it.openActions} actions`}</Pill>
        ) : (
          <Pill>Done</Pill>
        );
    }
  };

  const subtitle = (it: MeetingListItem) => {
    if (it.status === "failed") return it.failure?.message ?? "Summary failed. Transcript is saved.";
    if (it.status === "processing" && !it.takeaway) return `Working ${localPhrase(platform)}…`;
    return it.takeaway ?? undefined;
  };

  const today = new Date();
  const empty = !list.loading && !list.error && (list.data?.length ?? 0) === 0;

  return (
    <div className={s.page} data-screen="meetings" data-ready={String(!list.loading && !stats.loading)}>
      <div className={s.toolbar}>
        <SearchField
          ref={searchRef}
          className={s.search}
          placeholder="Search meetings, people, decisions"
          platform={platform}
          value={query}
          onChange={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && query.trim()) openSearch(go, query);
            if (e.key === "Escape") setQuery("");
          }}
        />
      </div>

      <PageHeader
        overline={longDate(today)}
        title="Meetings"
        right={
          narrow ? undefined : (
            <>
              <Stat
                value={stats.data ? formatHoursMinutes(stats.data.recordedMs) : <span className={s.statLoading}>—</span>}
                caption="Recorded this week"
              />
              <Stat
                value={stats.data ? stats.data.openActions : <span className={s.statLoading}>—</span>}
                caption="Open actions"
              />
            </>
          )
        }
      />

      {list.error && !list.data ? (
        <EmptyState
          className={s.errorBox}
          title="Couldn't load meetings."
          description={list.error.message}
          action={
            <Button variant="secondary" onClick={() => void list.refetch()}>
              Try again
            </Button>
          }
        />
      ) : empty ? (
        <EmptyState
          align="center"
          title="No meetings yet"
          description={`Join a call and Minutae will ask. Or press ${formatShortcut("mod+r", platform)} to record now.`}
        />
      ) : groups && groups.length === 0 ? (
        <EmptyState
          title={`Nothing matches “${query.trim()}”`}
          description="Press ⏎ to search transcripts in Library."
        />
      ) : (
        <div className={s.groups} aria-busy={list.loading || undefined}>
          {groups?.map((g) => (
            <section key={g.label} className={s.group} aria-label={g.label}>
              <GroupLabel as="h2" count={g.items.length}>
                {g.label}
              </GroupLabel>
              {g.items.map((it) => (
                <LaneRow
                  key={it.id}
                  lanes={narrow ? { people: false, duration: false, status: 96 } : undefined}
                  time={formatRelativeDate(new Date(it.startedAt), today)}
                  title={it.title}
                  subtitle={subtitle(it)}
                  people={people(it.people)}
                  duration={it.durationMs != null ? formatDuration(it.durationMs) : "—"}
                  status={status(it)}
                  onClick={() => go(it.status === "recording" ? "/live" : `/meetings/${it.id}`)}
                />
              ))}
            </section>
          ))}
        </div>
      )}
    </div>
  );
}
