import type { JSX, MouseEvent as ReactMouseEvent } from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { backend } from "../lib/backend";
import { errorMessage } from "../lib/errors";
import {
  type Metric, type PresetKey, type UnitKey,
  PRESETS, PRESET_KEYS, UNITS, UNIT_KEYS, WEEK_ALL,
  alignedSince, axisLabelsFor, bucketCount, bucketSeries, effectiveUnit,
  metricFromKey, metricKey, metricLabel, missingLabel, nearestKnownSlot, showMissing, slotLabel, unitAllowed,
} from "../lib/history";
import { polylineRuns, seriesDots, seriesStats } from "../lib/series";
import { metricColor } from "../lib/theme";
import type { HistoryPoint, SnapshotDto } from "../lib/types";

interface Props {
  accountId: string;
  latest: SnapshotDto | null;
  /** Bumped by useDashboard on every cycle:finished; a change refetches. */
  cycle: number;
  onError: (message: string) => void;
  onCollapse: () => void;
}

/** One resolved request. The chart renders ONLY from this, never from the pickers. */
interface Result { points: HistoryPoint[]; since: number; unit: UnitKey; count: number; fetchNow: number }
interface Tip { left: string; bottom: string; edge: "left" | "mid" | "right"; text: string }

const TIP_TRANSFORM: Record<Tip["edge"], string> = {
  right: "translate(-100%, calc(100% + 14px))",
  left: "translate(0, calc(100% + 14px))",
  mid: "translate(-50%, calc(100% + 14px))",
};
/** Dots are decorative; above this many slots they are skipped (hover still works). */
const MAX_DOTS = 200;
const AXIS_LABELS = 4;

function latestValue(latest: SnapshotDto | null, metric: Metric): number {
  if (latest === null) return 0;
  switch (metric.kind) {
    case "week_all": return latest.week_all?.pct ?? 0;
    case "session": return latest.session?.pct ?? 0;
    case "model": return latest.week_models.find((m) => m.label === metric.label)?.pct ?? 0;
  }
}

export function HistoryDrawer({ accountId, latest, cycle, onError, onCollapse }: Props): JSX.Element {
  const [preset, setPreset] = useState<PresetKey>("30d");
  const [unitOverride, setUnitOverride] = useState<UnitKey | null>(null);
  const [metric, setMetric] = useState<Metric>(WEEK_ALL);
  const [models, setModels] = useState<string[]>([]);
  const [result, setResult] = useState<Result | null>(null);
  const [loading, setLoading] = useState(false);
  const [tip, setTip] = useState<Tip | null>(null);
  // Only the most recently started request may write `result`.
  const seq = useRef(0);
  const onErrorRef = useRef(onError);
  useEffect(() => { onErrorRef.current = onError; }, [onError]);

  useEffect(() => {
    let cancelled = false;
    const load = async (): Promise<void> => {
      try {
        const labels = await backend().invoke<string[]>("get_history_models", { accountId });
        if (!cancelled) setModels(labels);
      } catch (e) {
        if (!cancelled) onErrorRef.current(errorMessage(e));
      }
    };
    void load();
    return () => { cancelled = true; };
  }, [accountId]);

  const unit = effectiveUnit(preset, unitOverride);

  useEffect(() => {
    const id = ++seq.current;
    const fetchNow = Date.now();
    const since = alignedSince(fetchNow, preset, unit);
    const count = bucketCount(since, fetchNow, unit);
    setLoading(true);
    const load = async (): Promise<void> => {
      try {
        const points = await backend().invoke<HistoryPoint[]>("get_history", {
          accountId, since, bucketMs: UNITS[unit].ms, metric,
        });
        if (id === seq.current) setResult({ points, since, unit, count, fetchNow });
      } catch (e) {
        if (id === seq.current) onErrorRef.current(errorMessage(e));
      } finally {
        if (id === seq.current) setLoading(false);
      }
    };
    void load();
  }, [accountId, preset, unit, metric, cycle]);

  const choosePreset = (p: PresetKey): void => {
    setPreset(p);
    if (unitOverride !== null && !unitAllowed(p, unitOverride)) setUnitOverride(null);
  };

  // Derived once per result, not per mouse move (a 720-slot series would
  // otherwise be re-bucketed and re-stringified on every pointer event).
  const { vals, stats, lines, dots } = useMemo(() => {
    const v: Array<number | null> = result === null ? [] : bucketSeries(result.points, result.since, result.unit, result.count);
    return {
      vals: v,
      stats: seriesStats(v),
      lines: polylineRuns(v, 100, 100),
      dots: v.length <= MAX_DOTS ? seriesDots(v) : [],
    };
  }, [result]);
  const stroke = metricColor(latestValue(latest, metric));
  const shownUnit = result?.unit ?? unit;
  const axis = result === null ? [] : axisLabelsFor(result.since, result.unit, result.count, AXIS_LABELS);
  const selectedKey = metricKey(metric);
  const options = metric.kind === "model" && !models.includes(metric.label) ? [...models, metric.label] : models;

  const onPlotMove = (e: ReactMouseEvent<HTMLDivElement>): void => {
    if (result === null) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const fraction = rect.width > 0 ? (e.clientX - rect.left) / rect.width : 0;
    const idx = nearestKnownSlot(vals, fraction);
    const value = idx === null ? null : vals[idx];
    if (idx === null || value === null || value === undefined) { setTip(null); return; }
    const leftPct = vals.length === 1 ? 0 : (idx / (vals.length - 1)) * 100;
    setTip({
      left: `${leftPct.toFixed(2)}%`,
      bottom: `${value}%`,
      edge: leftPct > 78 ? "right" : leftPct < 22 ? "left" : "mid",
      text: `${slotLabel(result.since, result.unit, idx)} · ${value}%`,
    });
  };

  return (
    <div className="drawer">
      <div className="drawer-head">
        <span className="drawer-label">
          Last {PRESETS[preset].label} · {metricLabel(metric)} · {shownUnit}{loading ? " · loading" : ""}
        </span>
        <div className="drawer-stats">
          <span>peak {stats.peak}%</span>
          <span>avg {stats.avg}%</span>
          {showMissing(shownUnit) && <span>missing {missingLabel(stats.missing, shownUnit)}</span>}
          <button type="button" className="btn-link" onClick={onCollapse}>collapse</button>
        </div>
      </div>
      <div className="drawer-controls">
        <div className="drawer-group" role="group" aria-label="Range">
          {PRESET_KEYS.map((p) => (
            <button key={p} type="button" className={`btn btn-sm${p === preset ? " btn-edit-on" : ""}`} onClick={() => choosePreset(p)}>{p}</button>
          ))}
        </div>
        <div className="drawer-group" role="group" aria-label="Granularity">
          <button type="button" className={`btn btn-sm${unitOverride === null ? " btn-edit-on" : ""}`} onClick={() => setUnitOverride(null)}>auto</button>
          {UNIT_KEYS.map((u) => (
            <button key={u} type="button" disabled={!unitAllowed(preset, u)}
              className={`btn btn-sm${unitOverride === u ? " btn-edit-on" : ""}`} onClick={() => setUnitOverride(u)}>{u}</button>
          ))}
        </div>
        <select className="input input-mono drawer-metric" aria-label="Metric" value={selectedKey}
          onChange={(e) => setMetric(metricFromKey(e.target.value))}>
          <option value="week_all">weekly limit</option>
          <option value="session">session</option>
          {options.map((label) => <option key={label} value={`model:${label}`}>{label}</option>)}
        </select>
      </div>
      <div className="chart">
        <svg viewBox="0 0 100 100" preserveAspectRatio="none" role="img" aria-label={`${metricLabel(metric)}, last ${PRESETS[preset].label}`}>
          {[0, 50, 100].map((y) => <line key={y} x1={0} y1={y} x2={100} y2={y} stroke="#1e252a" strokeWidth={1} vectorEffect="non-scaling-stroke" />)}
          {lines.map((points, i) => (
            <polyline key={i} points={points} fill="none" stroke={stroke} strokeWidth={1.8} vectorEffect="non-scaling-stroke" strokeLinejoin="round" strokeLinecap="round" />
          ))}
        </svg>
        <div className="chart-dots" onMouseMove={onPlotMove} onMouseLeave={() => setTip(null)}>
          {dots.map((d) => (
            <div key={d.index} className="dot" style={{ left: `${d.leftPct.toFixed(2)}%`, bottom: `${d.value}%` }} />
          ))}
          {tip !== null && (
            <div className="tip" style={{ left: tip.left, bottom: tip.bottom, transform: TIP_TRANSFORM[tip.edge] }}>{tip.text}</div>
          )}
        </div>
        <span className="chart-y chart-y-top">100%</span>
        <span className="chart-y chart-y-bottom">0%</span>
      </div>
      <div className="chart-axis">
        {axis.map((t, i) => <span key={i}>{t}</span>)}
      </div>
    </div>
  );
}
