function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

/**
 * One `points` string per contiguous run of known values, so a gap in the
 * data breaks the line instead of connecting across missing slots. A run of
 * length 1 is emitted as a single point (harmless as a `<polyline>`; the
 * drawer draws a dot for it when dots are on).
 */
export function polylineRuns(
  vals: readonly (number | null)[],
  width: number,
  height: number,
): string[] {
  const n = vals.length;
  const point = (i: number, v: number): string => {
    const x = n === 1 ? 0 : (i / (n - 1)) * width;
    const y = height - (clamp(v, 0, 100) / 100) * height;
    return `${x.toFixed(2)},${y.toFixed(2)}`;
  };
  const runs: string[] = [];
  let current: string[] = [];
  vals.forEach((v, i) => {
    if (v === null) {
      if (current.length > 0) { runs.push(current.join(" ")); current = []; }
      return;
    }
    current.push(point(i, v));
  });
  if (current.length > 0) runs.push(current.join(" "));
  return runs;
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
