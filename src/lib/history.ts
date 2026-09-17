import limits from "./historyLimits.json";
import type { HistoryPoint } from "./types";

/** Shared with src-tauri/src/store/snapshots.rs via historyLimits.json. */
export const MIN_BUCKET_MS: number = limits.minBucketMs;
export const MAX_BUCKETS: number = limits.maxBuckets;
export const MAX_RANGE_MS: number = limits.maxRangeMs;

export type PresetKey = "1h" | "6h" | "12h" | "24h" | "7d" | "30d";
export type UnitKey = "1m" | "5m" | "15m" | "1h" | "1d";

export interface Preset { label: string; rangeMs: number; autoUnit: UnitKey }
export interface Unit { label: string; ms: number }

const MINUTE = 60_000;
const HOUR = 3_600_000;
const DAY = 86_400_000;

export const UNITS: Record<UnitKey, Unit> = {
  "1m": { label: "1m", ms: MINUTE },
  "5m": { label: "5m", ms: 5 * MINUTE },
  "15m": { label: "15m", ms: 15 * MINUTE },
  "1h": { label: "1h", ms: HOUR },
  "1d": { label: "1d", ms: DAY },
};
export const UNIT_KEYS: readonly UnitKey[] = ["1m", "5m", "15m", "1h", "1d"];

export const PRESETS: Record<PresetKey, Preset> = {
  "1h": { label: "1 h", rangeMs: HOUR, autoUnit: "1m" },
  "6h": { label: "6 h", rangeMs: 6 * HOUR, autoUnit: "5m" },
  "12h": { label: "12 h", rangeMs: 12 * HOUR, autoUnit: "15m" },
  "24h": { label: "24 h", rangeMs: DAY, autoUnit: "15m" },
  "7d": { label: "7 days", rangeMs: 7 * DAY, autoUnit: "1h" },
  "30d": { label: "30 days", rangeMs: 30 * DAY, autoUnit: "1d" },
};
export const PRESET_KEYS: readonly PresetKey[] = ["1h", "6h", "12h", "24h", "7d", "30d"];

/** Wire shape of the Rust `HistoryMetric` (serde tag = "kind"). */
export type Metric =
  | { kind: "week_all" }
  | { kind: "session" }
  | { kind: "model"; label: string };

export const WEEK_ALL: Metric = { kind: "week_all" };

export function metricLabel(m: Metric): string {
  switch (m.kind) {
    case "week_all": return "weekly limit";
    case "session": return "session";
    case "model": return m.label;
  }
}

/** Stable string for React keys and <select> values. */
export function metricKey(m: Metric): string {
  return m.kind === "model" ? `model:${m.label}` : m.kind;
}

/** Inverse of metricKey; anything unrecognised falls back to week-all. */
export function metricFromKey(key: string): Metric {
  if (key === "session") return { kind: "session" };
  if (key.startsWith("model:") && key.length > "model:".length) {
    return { kind: "model", label: key.slice("model:".length) };
  }
  return WEEK_ALL;
}

/** At least two buckets and at most MAX_BUCKETS (spec §3.2). */
export function unitAllowed(preset: PresetKey, unit: UnitKey): boolean {
  const range = PRESETS[preset].rangeMs;
  const ms = UNITS[unit].ms;
  return range / ms >= 2 && Math.ceil(range / ms) <= MAX_BUCKETS;
}

export function effectiveUnit(preset: PresetKey, override: UnitKey | null): UnitKey {
  return override !== null && unitAllowed(preset, override) ? override : PRESETS[preset].autoUnit;
}

/**
 * `now - range`, rounded UP to the next unit boundary in local time (a value
 * already on a boundary stays put). Rounding up keeps `now - since <= range`
 * so the server's 30-day limit can never be crossed; the partial first unit
 * is dropped, not stretched.
 */
export function alignedSince(now: number, preset: PresetKey, unit: UnitKey): number {
  const raw = now - PRESETS[preset].rangeMs;
  if (unit === "1d") {
    const d = new Date(raw);
    d.setHours(0, 0, 0, 0);
    if (d.getTime() < raw) d.setDate(d.getDate() + 1);
    return d.getTime();
  }
  const hourStart = new Date(raw);
  hourStart.setMinutes(0, 0, 0);
  const ms = UNITS[unit].ms;
  const offset = raw - hourStart.getTime();
  return hourStart.getTime() + Math.ceil(offset / ms) * ms;
}

/** Slots from `since` up to and including the current, partial unit. */
export function bucketCount(since: number, now: number, unit: UnitKey): number {
  return Math.max(1, Math.ceil((now - since) / UNITS[unit].ms));
}

/** slot i = max pct of points with t in [since + i*ms, since + (i+1)*ms); null when none. */
export function bucketSeries(
  points: readonly HistoryPoint[],
  since: number,
  unit: UnitKey,
  count: number,
): Array<number | null> {
  const ms = UNITS[unit].ms;
  const out: Array<number | null> = Array.from({ length: count }, () => null);
  for (const p of points) {
    const idx = Math.floor((p.t - since) / ms);
    if (idx < 0 || idx >= count) continue;
    const cur = out[idx];
    out[idx] = cur === null ? p.pct : Math.max(cur, p.pct);
  }
  return out;
}

/** Start instant of slot `index`; day slots step by calendar day (DST-safe). */
function slotStart(since: number, unit: UnitKey, index: number): number {
  if (unit === "1d") {
    const d = new Date(since);
    d.setDate(d.getDate() + index);
    return d.getTime();
  }
  return since + index * UNITS[unit].ms;
}

function dateText(t: number, locale?: string): string {
  return new Date(t).toLocaleDateString(locale, { month: "short", day: "numeric" });
}

function timeText(t: number, locale?: string): string {
  return new Date(t).toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
}

/** `labels` evenly spaced slot labels, first and last always included. */
export function axisLabelsFor(
  since: number,
  unit: UnitKey,
  count: number,
  labels: number,
  locale?: string,
): string[] {
  // Never more labels than slots, or a short series repeats a label.
  const c = Math.max(2, Math.min(labels, count));
  const useDate = unit === "1d" || count * UNITS[unit].ms >= 2 * DAY;
  return Array.from({ length: c }, (_, i) => {
    const index = Math.round((i * (count - 1)) / (c - 1));
    const t = slotStart(since, unit, index);
    return useDate ? dateText(t, locale) : timeText(t, locale);
  });
}

export function slotLabel(since: number, unit: UnitKey, index: number, locale?: string): string {
  const t = slotStart(since, unit, index);
  return unit === "1d" ? dateText(t, locale) : `${dateText(t, locale)} ${timeText(t, locale)}`;
}

/** "12 m" | "3 h" | "2 d" for the stats strip. */
export function missingLabel(missing: number, unit: UnitKey): string {
  switch (unit) {
    case "1m": return `${missing} m`;
    case "5m": return `${missing * 5} m`;
    case "15m": return `${missing * 15} m`;
    case "1h": return `${missing} h`;
    case "1d": return `${missing} d`;
  }
}

/** Poll density is one row per 1.7–3.4 min, so at 1m gaps are expected, not missing. */
export function showMissing(unit: UnitKey): boolean {
  return unit !== "1m";
}

/**
 * polylineRuns places slot i at x = i/(n-1), so the candidate is
 * round(fraction * (n-1)); from there the nearest non-null slot wins, the
 * lower index on ties. Null when every slot is null.
 */
export function nearestKnownSlot(vals: readonly (number | null)[], fraction: number): number | null {
  const n = vals.length;
  if (n === 0) return null;
  const f = Math.min(1, Math.max(0, fraction));
  const start = n === 1 ? 0 : Math.round(f * (n - 1));
  if (vals[start] !== null) return start;
  for (let d = 1; d < n; d++) {
    const lo = start - d;
    const hi = start + d;
    if (lo >= 0 && vals[lo] !== null) return lo;
    if (hi < n && vals[hi] !== null) return hi;
  }
  return null;
}
