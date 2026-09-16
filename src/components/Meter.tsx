import type { JSX } from "react";
import { metricColor } from "../lib/theme";

interface Props { pct: number | null; note: string; noteWarn?: boolean; title?: string }

export function Meter({ pct, note, noteWarn = false, title }: Props): JSX.Element {
  const clamped = pct === null ? 0 : Math.max(0, Math.min(100, pct));
  return (
    <div className="meter" title={title}>
      <div className="meter-track">
        {pct !== null && (
          <div className="meter-fill" style={{ width: `${clamped}%`, background: metricColor(clamped) }} />
        )}
      </div>
      <div className="meter-meta">
        <span className="meter-pct">{pct === null ? "—" : `${Math.round(clamped)}%`}</span>
        <span className="meter-sep">·</span>
        <span className={`meter-note${noteWarn ? " meter-note-warn" : ""}`}>{note}</span>
      </div>
    </div>
  );
}
