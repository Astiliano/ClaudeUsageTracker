/** Data colours from docs/design/usage-tracker-kit.js (THEME). */
export const THEME = {
  ok: "#7aa2f7",
  warn: "#d8a94f",
  crit: "#e0705f",
  live: "#5fb98a",
  idle: "#4b5359",
  meta: "#8b949e",
  metaWarn: "#d08c80",
} as const;

export const THRESHOLDS = { warn: 70, crit: 95 } as const;

export type MetricTone = "ok" | "warn" | "crit";

export function metricTone(pct: number): MetricTone {
  if (pct >= THRESHOLDS.crit) return "crit";
  if (pct >= THRESHOLDS.warn) return "warn";
  return "ok";
}

export function metricColor(pct: number): string {
  return THEME[metricTone(pct)];
}
