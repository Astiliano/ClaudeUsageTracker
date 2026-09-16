import type { JSX } from "react";
import { buildSparklinePaths } from "../lib/sparkline";
import type { HistoryPoint } from "../lib/types";

interface Props {
  points: HistoryPoint[];
  stroke: string;
}

/**
 * Hand-rolled SVG, no chart library. Each sub-path is one contiguous run of
 * hours; gaps between runs are simply not drawn.
 */
export function Sparkline({ points, stroke }: Props): JSX.Element {
  const paths = buildSparklinePaths(points, 100, 24);

  if (paths.length === 0) {
    return <span className="spark-empty">—</span>;
  }

  return (
    <svg
      viewBox="0 0 100 24"
      preserveAspectRatio="none"
      role="img"
      aria-label="weekly usage, last 7 days"
    >
      {paths.map((d, i) => (
        <path
          key={`${i}-${d.slice(0, 16)}`}
          d={d}
          fill="none"
          stroke={stroke}
          strokeWidth={1.6}
          strokeLinecap="round"
          strokeLinejoin="round"
          vectorEffect="non-scaling-stroke"
        />
      ))}
    </svg>
  );
}
