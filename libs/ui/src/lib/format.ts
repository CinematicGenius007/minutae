function pad2(n: number): string {
  return n < 10 ? `0${n}` : String(n);
}

/**
 * Duration as a clock: 1_934_000 → "32:14"; hours appear only when needed:
 * 4_334_000 → "1:12:14". Negative and non-finite inputs read as 0.
 */
export function formatDuration(ms: number): string {
  const total = Number.isFinite(ms) && ms > 0 ? Math.floor(ms / 1000) : 0;
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h > 0 ? `${h}:${pad2(m)}:${pad2(s)}` : `${pad2(m)}:${pad2(s)}`;
}

/** Wall-clock time of day: "14:30" (24 h) or "2:30 PM" (12 h). */
export function formatClock(date: Date, hour12 = false): string {
  const h = date.getHours();
  const m = pad2(date.getMinutes());
  if (!hour12) return `${pad2(h)}:${m}`;
  const suffix = h < 12 ? "AM" : "PM";
  const h12 = h % 12 === 0 ? 12 : h % 12;
  return `${h12}:${m} ${suffix}`;
}

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/**
 * Relative stamp for list rows (§9): "14:30" today, "Wed" within the last
 * week, "12 Nov" otherwise.
 */
export function formatRelativeDate(date: Date, now: Date = new Date(), hour12 = false): string {
  const days = Math.round((startOfDay(now) - startOfDay(date)) / 86_400_000);
  if (days === 0) return formatClock(date, hour12);
  if (days > 0 && days < 7) return WEEKDAYS[date.getDay()] as string;
  return `${date.getDate()} ${MONTHS[date.getMonth()]}`;
}

/** Bytes as "466 MB" (§9). Uses 1000-based units, no decimals above 10. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 1000) return `${Math.max(0, Math.round(bytes || 0))} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = -1;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

/** Clamps progress to 0–1. */
export function clamp01(v: number): number {
  if (!Number.isFinite(v)) return 0;
  return Math.min(1, Math.max(0, v));
}
