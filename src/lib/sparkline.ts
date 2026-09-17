import type { HistoryPoint } from "./types";

function round(n: number): number {
  return Math.round(n * 100) / 100;
}

/**
 * Hand-rolled SVG path data for the row sparkline: one continuous path
 * through every point, sorted by time, x from the first to the last point.
 * Empty buckets are gaps in sampling (Claude was idle), not readings, so
 * they are neither drawn as zeros nor allowed to break the line. Null when
 * there is nothing to draw.
 */
export function buildSparklinePath(
  points: HistoryPoint[],
  width: number,
  height: number,
): string | null {
  if (points.length === 0) {
    return null;
  }

  const sorted = [...points].sort((a, b) => a.t - b.t);
  const first = sorted[0].t;
  const last = sorted[sorted.length - 1].t;
  const span = last - first;

  const x = (t: number): number =>
    span === 0 ? width : round(((t - first) / span) * width);
  const y = (pct: number): number =>
    round(height - (Math.min(Math.max(pct, 0), 100) / 100) * height);

  const head = sorted[0];
  const start = `M ${x(head.t)} ${y(head.pct)}`;
  if (sorted.length === 1) {
    // A lone reading still has to be visible: a zero-length line that the
    // round caps render as a dot.
    return `${start} L ${x(head.t)} ${y(head.pct)}`;
  }
  const rest = sorted
    .slice(1)
    .map((p) => `L ${x(p.t)} ${y(p.pct)}`)
    .join(" ");
  return `${start} ${rest}`;
}
