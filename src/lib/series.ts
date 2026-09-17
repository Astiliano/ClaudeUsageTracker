function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

/**
 * One SVG `points` string through every known value. Slot i sits at
 * x = i/(n-1) * width (0 when n === 1); a null slot is skipped, so the line
 * runs straight from the last known value to the next one. Empty buckets
 * are gaps in sampling, not readings, so they are neither zeros nor breaks.
 * "" when nothing is known.
 */
export function seriesLine(
  vals: readonly (number | null)[],
  width: number,
  height: number,
): string {
  const n = vals.length;
  const points: string[] = [];
  vals.forEach((v, i) => {
    if (v === null) return;
    const x = n === 1 ? 0 : (i / (n - 1)) * width;
    const y = height - (clamp(v, 0, 100) / 100) * height;
    points.push(`${x.toFixed(2)},${y.toFixed(2)}`);
  });
  return points.join(" ");
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
