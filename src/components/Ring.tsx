import type { JSX } from "react";
import { RING_SIZES, ringDash, ringViewBox } from "../lib/gauge";
import { metricColor } from "../lib/theme";

interface Props { pct: number | null; title?: string }

/**
 * A single-value meter bent into a circle: track in the grid colour, arc in
 * the same status colour the linear Meter uses. Arc starts at 12 o'clock.
 *
 * It draws track and arc only and is aria-hidden: it sits beside its own
 * label on the system line, which is the accessible name.
 */
export function Ring({ pct, title }: Props): JSX.Element {
  const geometry = RING_SIZES.sm;
  const { circumference, offset } = ringDash(pct, geometry.radius);
  const c = geometry.size / 2;

  return (
    <span className="ring-sm" title={title} aria-hidden="true">
      <svg viewBox={ringViewBox(geometry)} aria-hidden="true">
        <circle cx={c} cy={c} r={geometry.radius} fill="none" stroke="var(--track-off)" strokeWidth={geometry.stroke} />
        {pct !== null && pct > 0 && (
          <circle cx={c} cy={c} r={geometry.radius} fill="none" stroke={metricColor(pct)} strokeWidth={geometry.stroke}
            strokeLinecap="round" strokeDasharray={circumference} strokeDashoffset={offset}
            transform={`rotate(-90 ${c} ${c})`} />
        )}
      </svg>
    </span>
  );
}
