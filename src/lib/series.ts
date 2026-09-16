import type { HistoryPoint } from "./types";

const DAY_MS = 86_400_000;

function startOfLocalDay(t: number): number {
  const d = new Date(t);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

/**
 * Daily buckets of the max hourly reading, last `days` local calendar days,
 * newest last. Rounding the day distance absorbs 23/25-hour DST days.
 */
export function dailyMax(
  points: readonly HistoryPoint[],
  days: number,
  now: number,
): Array<number | null> {
  const today = startOfLocalDay(now);
  const out: Array<number | null> = Array.from({ length: days }, () => null);
  for (const p of points) {
    const back = Math.round((today - startOfLocalDay(p.t)) / DAY_MS);
    const idx = days - 1 - back;
    if (idx < 0 || idx >= days) continue;
    const cur = out[idx];
    out[idx] = cur === null ? p.pct : Math.max(cur, p.pct);
  }
  return out;
}

/** Connected polyline through the known values (design: nulls are skipped). */
export function polylinePoints(
  vals: readonly (number | null)[],
  width: number,
  height: number,
): string {
  const n = vals.length;
  const pts: string[] = [];
  vals.forEach((v, i) => {
    if (v === null) return;
    const x = n === 1 ? 0 : (i / (n - 1)) * width;
    const y = height - (clamp(v, 0, 100) / 100) * height;
    pts.push(`${x.toFixed(2)},${y.toFixed(2)}`);
  });
  return pts.length > 1 ? pts.join(" ") : "";
}

export interface SeriesDot { index: number; value: number; leftPct: number }

export function seriesDots(vals: readonly (number | null)[]): SeriesDot[] {
  const n = vals.length;
  const out: SeriesDot[] = [];
  vals.forEach((v, i) => {
    if (v === null) return;
    out.push({ index: i, value: v, leftPct: n === 1 ? 0 : (i / (n - 1)) * 100 });
  });
  return out;
}

export interface SeriesStats { peak: number; avg: number; missing: number }

export function seriesStats(vals: readonly (number | null)[]): SeriesStats {
  const known = vals.filter((v): v is number => v !== null);
  const sum = known.reduce((a, b) => a + b, 0);
  return {
    peak: known.length ? Math.max(...known) : 0,
    avg: known.length ? Math.round(sum / known.length) : 0,
    missing: vals.length - known.length,
  };
}

export function dayLabel(index: number, total: number, now: number, locale?: string): string {
  const d = new Date(now);
  d.setDate(d.getDate() - (total - 1 - index));
  return d.toLocaleDateString(locale, { month: "short", day: "numeric" });
}

export function axisLabels(total: number, count: number, now: number, locale?: string): string[] {
  const c = Math.max(2, count);
  return Array.from({ length: c }, (_, i) =>
    dayLabel(Math.round((i * (total - 1)) / (c - 1)), total, now, locale),
  );
}
