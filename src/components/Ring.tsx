import type { JSX } from "react";
import { RING_SIZES, ringDash } from "../lib/gauge";
import { metricColor } from "../lib/theme";

type Props = { pct: number | null; title?: string } & (
  | { size?: "md"; label: string }
  | { size: "sm" }
);

/**
 * A single-value meter bent into a circle: track in the grid colour, arc in
 * the same status colour the linear Meter uses, value in text ink (never
 * the series colour), label under. Arc starts at 12 o'clock.
 *
 * The `sm` variant draws track and arc only and is aria-hidden: it sits
 * beside its own label on the system line, which is the accessible name.
 */
export function Ring(props: Props): JSX.Element {
  const { pct, title } = props;
  const small = props.size === "sm";
  const geometry = small ? RING_SIZES.sm : RING_SIZES.md;
  const { circumference, offset } = ringDash(pct, geometry.radius);
  const c = geometry.size / 2;
  const text = pct === null ? "—" : `${Math.round(Math.max(0, Math.min(100, pct)))}%`;

  const arc = (
    <svg
      width={geometry.size}
      height={geometry.size}
      viewBox={`0 0 ${geometry.size} ${geometry.size}`}
      aria-hidden="true"
    >
      <circle cx={c} cy={c} r={geometry.radius} fill="none" stroke="var(--track-off)" strokeWidth={geometry.stroke} />
      {pct !== null && pct > 0 && (
        <circle cx={c} cy={c} r={geometry.radius} fill="none" stroke={metricColor(pct)} strokeWidth={geometry.stroke}
          strokeLinecap="round" strokeDasharray={circumference} strokeDashoffset={offset}
          transform={`rotate(-90 ${c} ${c})`} />
      )}
      {!small && (
        <text x={c} y={c} className="ring-value" textAnchor="middle" dominantBaseline="central">{text}</text>
      )}
    </svg>
  );

  if (small) {
    return <span className="ring-sm" title={title} aria-hidden="true">{arc}</span>;
  }

  return (
    <div className="ring" title={title} role="img" aria-label={`${props.label} ${text}`}>
      {arc}
      <span className="ring-label">{props.label}</span>
    </div>
  );
}
