import type { HistoryPoint } from "./types";

const HOUR = 3_600_000;

function round(n: number): number {
  return Math.round(n * 100) / 100;
}

/**
 * Hand-rolled SVG path data for the hourly week-all series.
 *
 * Missing hours are breaks, never zeros: the process gate guarantees
 * overnight gaps, so any hour more than one bucket after its predecessor
 * starts a new sub-path. The caller renders one <path> per string.
 */
export function buildSparklinePaths(
  points: HistoryPoint[],
  width: number,
  height: number,
): string[] {
  if (points.length === 0) {
    return [];
  }

  const sorted = [...points].sort((a, b) => a.t - b.t);
  const first = sorted[0].t;
  const last = sorted[sorted.length - 1].t;
  const span = last - first;

  const x = (t: number): number =>
    span === 0 ? width : round(((t - first) / span) * width);
  const y = (pct: number): number =>
    round(height - (Math.min(Math.max(pct, 0), 100) / 100) * height);

  const segments: HistoryPoint[][] = [];
  let current: HistoryPoint[] = [sorted[0]];

  for (let i = 1; i < sorted.length; i += 1) {
    const gap = sorted[i].t - sorted[i - 1].t;
    if (gap > HOUR) {
      segments.push(current);
      current = [sorted[i]];
    } else {
      current.push(sorted[i]);
    }
  }
  segments.push(current);

  return segments.map((segment) => {
    const head = segment[0];
    const start = `M ${x(head.t)} ${y(head.pct)}`;
    if (segment.length === 1) {
      // A lone reading still has to be visible, so draw a zero-length line.
      return `${start} L ${x(head.t)} ${y(head.pct)}`;
    }
    const rest = segment
      .slice(1)
      .map((p) => `L ${x(p.t)} ${y(p.pct)}`)
      .join(" ");
    return `${start} ${rest}`;
  });
}
