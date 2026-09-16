import type { JSX } from "react";
import { useState } from "react";
import { HISTORY_DAYS } from "../hooks/useDashboard";
import { axisLabels, dailyMax, dayLabel, polylinePoints, seriesDots, seriesStats } from "../lib/series";
import type { HistoryPoint } from "../lib/types";

interface Props { points: HistoryPoint[]; now: number; stroke: string; onCollapse: () => void }
interface Tip { left: string; bottom: string; edge: "left" | "mid" | "right"; text: string }

const TIP_TRANSFORM: Record<Tip["edge"], string> = {
  right: "translate(-100%, calc(100% + 14px))",
  left: "translate(0, calc(100% + 14px))",
  mid: "translate(-50%, calc(100% + 14px))",
};

export function HistoryDrawer({ points, now, stroke, onCollapse }: Props): JSX.Element {
  const [tip, setTip] = useState<Tip | null>(null);
  const days = dailyMax(points, HISTORY_DAYS, now);
  const stats = seriesStats(days);
  const line = polylinePoints(days, 100, 100);
  const dots = seriesDots(days);
  return (
    <div className="drawer">
      <div className="drawer-head">
        <span className="drawer-label">Last 30 days · weekly limit use</span>
        <div className="drawer-stats">
          <span>peak {stats.peak}%</span>
          <span>avg {stats.avg}%</span>
          <span>missing {stats.missing} d</span>
          <button type="button" className="btn-link" onClick={onCollapse}>collapse</button>
        </div>
      </div>
      <div className="chart">
        <svg viewBox="0 0 100 100" preserveAspectRatio="none" role="img" aria-label="weekly usage, last 30 days">
          {[0, 50, 100].map((y) => <line key={y} x1={0} y1={y} x2={100} y2={y} stroke="#1e252a" strokeWidth={1} vectorEffect="non-scaling-stroke" />)}
          {line !== "" && <polyline points={line} fill="none" stroke={stroke} strokeWidth={1.8} vectorEffect="non-scaling-stroke" strokeLinejoin="round" strokeLinecap="round" />}
        </svg>
        <div className="chart-dots">
          {dots.map((d) => {
            const left = `${d.leftPct.toFixed(2)}%`;
            const bottom = `${d.value}%`;
            const edge = d.leftPct > 78 ? "right" : d.leftPct < 22 ? "left" : "mid";
            const text = `${dayLabel(d.index, HISTORY_DAYS, now)} · ${d.value}%`;
            return (
              <div key={d.index} className="dot" style={{ left, bottom }}
                onMouseEnter={() => setTip({ left, bottom, edge, text })}
                onMouseLeave={() => setTip(null)} />
            );
          })}
          {tip !== null && (
            <div className="tip" style={{ left: tip.left, bottom: tip.bottom, transform: TIP_TRANSFORM[tip.edge] }}>{tip.text}</div>
          )}
        </div>
        <span className="chart-y chart-y-top">100%</span>
        <span className="chart-y chart-y-bottom">0%</span>
      </div>
      <div className="chart-axis">
        {axisLabels(HISTORY_DAYS, 4, now).map((t) => <span key={t}>{t}</span>)}
      </div>
    </div>
  );
}
