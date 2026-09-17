import type { JSX } from "react";
import { RING, ringDash } from "../lib/gauge";
import { metricColor } from "../lib/theme";

interface Props { pct: number | null; label: string; title?: string }

/**
 * A single-value meter bent into a circle: track in the grid colour, arc in
 * the same status colour the linear Meter uses, value in text ink (never
 * the series colour), label under. Arc starts at 12 o'clock.
 */
export function Ring({ pct, label, title }: Props): JSX.Element {
  const { circumference, offset } = ringDash(pct, RING.radius);
  const c = RING.size / 2;
  const text = pct === null ? "—" : `${Math.round(Math.max(0, Math.min(100, pct)))}%`;
  return (
    <div className="ring" title={title} role="img" aria-label={`${label} ${text}`}>
      <svg width={RING.size} height={RING.size} viewBox={`0 0 ${RING.size} ${RING.size}`} aria-hidden="true">
        <circle cx={c} cy={c} r={RING.radius} fill="none" stroke="var(--track-off)" strokeWidth={RING.stroke} />
        {pct !== null && pct > 0 && (
          <circle cx={c} cy={c} r={RING.radius} fill="none" stroke={metricColor(pct)} strokeWidth={RING.stroke}
            strokeLinecap="round" strokeDasharray={circumference} strokeDashoffset={offset}
            transform={`rotate(-90 ${c} ${c})`} />
        )}
        <text x={c} y={c} className="ring-value" textAnchor="middle" dominantBaseline="central">{text}</text>
      </svg>
      <span className="ring-label">{label}</span>
    </div>
  );
}
