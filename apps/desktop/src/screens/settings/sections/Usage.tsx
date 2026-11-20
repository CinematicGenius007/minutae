import { Button, EmptyState, Spinner, cx } from "@minutae/ui";
import { api } from "../../../lib/api";
import { formatCount, formatUsd } from "../shared/format";
import { useLoad } from "../../../hooks";
import p from "../shared/page.module.css";
import s from "../settings.module.css";

const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const SHORT = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

export function UsageSection() {
  const now = new Date();
  const year = now.getFullYear();
  const month = now.getMonth() + 1;
  const [usage, reload] = useLoad(() => api.usageMonth(year, month), [year, month]);

  if (usage.status === "loading") {
    return (
      <div className={p.loading}>
        <Spinner /> Adding up this month…
      </div>
    );
  }
  if (!usage.data) {
    return (
      <EmptyState
        title="Couldn't load usage."
        description={usage.error ?? undefined}
        action={
          <Button variant="secondary" onClick={() => void reload()}>
            Try again
          </Button>
        }
      />
    );
  }

  const u = usage.data;
  const daysInMonth = new Date(u.year, u.month, 0).getDate();
  const isCurrent = u.year === year && u.month === month;
  const today = isCurrent ? now.getDate() : daysInMonth;
  const byDay = new Map(u.byDay.map((d) => [d.day, d.costUsd]));
  const max = Math.max(0.0001, ...u.byDay.map((d) => d.costUsd));
  const short = SHORT[u.month - 1] ?? "";
  const todayCost = byDay.get(today) ?? 0;

  return (
    <div className={s.usageGroups}>
      <section className={s.hero} aria-label="This month">
        <div className={s.heroFigure}>
          <span className={s.overline}>
            {MONTHS[u.month - 1]} · AI spend
          </span>
          <span className={s.spend}>{formatUsd(u.totalUsd)}</span>
        </div>
        <div className={s.heroStats}>
          <div className={s.miniStat}>
            <span className={s.miniValue}>{u.summaries.toLocaleString()}</span>
            <span className={s.miniCaption}>Summaries</span>
          </div>
          <div className={s.miniStat}>
            <span className={s.miniValue}>{formatCount(u.tokens)}</span>
            <span className={s.miniCaption}>Tokens</span>
          </div>
          <div className={s.miniStat}>
            <span className={s.miniValue}>{Math.round(u.freeShare * 100)}%</span>
            <span className={s.miniCaption}>Free (on device)</span>
          </div>
        </div>
      </section>

      <section className={s.chart} aria-label="Daily spend">
        <div className={s.bars} role="img" aria-label={`Daily AI spend for ${MONTHS[u.month - 1]}. Highest day ${formatUsd(max)}.`}>
          {Array.from({ length: daysInMonth }, (_, i) => {
            const day = i + 1;
            const future = day > today;
            const cost = byDay.get(day) ?? 0;
            const h = future ? 6 : cost > 0 ? Math.max(2, Math.round((cost / max) * 84)) : 0;
            return (
              <span
                key={day}
                className={cx(s.day, future && s.dayFuture)}
                style={{ height: h }}
                title={future ? undefined : `${day} ${short} · ${formatUsd(cost)}`}
              />
            );
          })}
        </div>
        <div className={s.axis}>
          <span>1 {short}</span>
          <span>{isCurrent ? `Today · ${formatUsd(todayCost)}` : ""}</span>
          <span>
            {daysInMonth} {short}
          </span>
        </div>
      </section>

      {u.byProvider.length === 0 ? (
        <EmptyState title="No AI calls this month." description="Everything so far ran on this device, for free." />
      ) : (
        <table className={s.table}>
          <thead>
            <tr>
              <th scope="col">By provider</th>
              <th scope="col" className={s.colCalls}>
                Calls
              </th>
              <th scope="col" className={s.colTokens}>
                Tokens
              </th>
              <th scope="col" className={s.colCost}>
                Cost
              </th>
            </tr>
          </thead>
          <tbody>
            {u.byProvider.map((r) => (
              <tr key={r.provider}>
                <td>{r.provider}</td>
                <td>{r.calls.toLocaleString()}</td>
                <td className={r.tokens === 0 ? s.dim : undefined}>{r.tokens === 0 ? "—" : formatCount(r.tokens)}</td>
                <td className={r.costUsd === 0 ? s.dim : undefined}>{r.costUsd === 0 ? "Free" : formatUsd(r.costUsd)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
