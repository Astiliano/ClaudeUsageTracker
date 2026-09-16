import type { JSX } from "react";
import { buildSparklinePaths } from "../lib/sparkline";
import type { HistoryPoint } from "../lib/types";

interface Props {
  points: HistoryPoint[];
  width?: number;
  height?: number;
}

/**
 * Hand-rolled SVG, no chart library. Each sub-path is one contiguous run of
 * hours; gaps between runs are simply not drawn.
 */
export function Sparkline({
  points,
  width = 120,
  height = 24,
}: Props): JSX.Element {
  const paths = buildSparklinePaths(points, width, height);

  if (paths.length === 0) {
    return <span className="sparkline-empty">—</span>;
  }

  return (
    <svg
      className="sparkline"
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      role="img"
      aria-label="weekly usage, last 7 days"
    >
      {paths.map((d, i) => (
        <path
          key={`${i}-${d.slice(0, 16)}`}
          d={d}
          fill="none"
          strokeWidth={1.5}
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      ))}
    </svg>
  );
}
