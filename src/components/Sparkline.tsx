import type { JSX } from "react";
import { buildSparklinePath } from "../lib/sparkline";
import type { HistoryPoint } from "../lib/types";

interface Props {
  points: HistoryPoint[];
  stroke: string;
}

/** Hand-rolled SVG, no chart library: one continuous path through every bucket. */
export function Sparkline({ points, stroke }: Props): JSX.Element {
  const d = buildSparklinePath(points, 100, 24);

  if (d === null) {
    return <span className="spark-empty">—</span>;
  }

  return (
    <svg
      viewBox="0 0 100 24"
      preserveAspectRatio="none"
      role="img"
      aria-label="weekly usage, last 7 days"
    >
      <path
        d={d}
        fill="none"
        stroke={stroke}
        strokeWidth={1.6}
        strokeLinecap="round"
        strokeLinejoin="round"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}
