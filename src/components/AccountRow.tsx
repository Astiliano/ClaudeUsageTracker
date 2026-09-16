import type { CSSProperties, JSX, KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from "react";
import { type ColumnKey } from "../lib/columns";
import { rowShift } from "../lib/drag";
import { formatAgo } from "../lib/format";
import { type Pill, statusPill } from "../lib/pill";
import { accountDotColor, sessionNote, summarizeModels, weekNote } from "../lib/present";
import { metricColor } from "../lib/theme";
import type { AccountRow as AccountRowData, HistoryPoint } from "../lib/types";
import type { RowDragState } from "./AccountsTable";
import { EditDrawer } from "./EditDrawer";
import { HistoryDrawer } from "./HistoryDrawer";
import { Meter } from "./Meter";
import { Sparkline } from "./Sparkline";

interface Props {
  row: AccountRowData;
  index: number;
  total: number;
  points: HistoryPoint[];
  now: number;
  columnOrder: readonly ColumnKey[];
  gridCols: string;
  hotColumn: number | null;
  drag: RowDragState | null;
  rowH: number;
  chartOpen: boolean;
  editing: boolean;
  onHandleDown: (e: ReactPointerEvent<HTMLDivElement>) => void;
  onToggleChart: () => void;
  onToggleEdit: () => void;
  onMove: (delta: number) => void;
  onChanged: () => void;
  onError: (m: string) => void;
  onShowFailure: (id: number) => void;
}

/** t >= now - 7 days; trivial enough not to need its own test. */
function last7(points: HistoryPoint[], now: number): HistoryPoint[] {
  const cutoff = now - 7 * 86_400_000;
  return points.filter((p) => p.t >= cutoff);
}

function StatusPill({ pill, onShowFailure }: { pill: Pill; onShowFailure: (id: number) => void }): JSX.Element {
  const cls = `pill pill-${pill.kind} pill-tone-${pill.tone}`;
  const id = pill.snapshotId;
  if (id !== undefined && pill.outcome !== "ok") {
    return (
      <button type="button" className={cls} title={pill.tooltip} onClick={() => onShowFailure(id)}>
        {pill.label}
      </button>
    );
  }
  return <span className={cls} title={pill.tooltip}>{pill.label}</span>;
}

export function AccountRow(props: Props): JSX.Element {
  const {
    row, index, total, points, now, columnOrder, gridCols, hotColumn, drag, rowH,
    chartOpen, editing, onHandleDown, onToggleChart, onToggleEdit, onMove, onChanged, onError, onShowFailure,
  } = props;

  const pill = statusPill(row, now);
  const session = row.latest?.session ?? null;
  const week = row.latest?.week_all ?? null;
  const models = summarizeModels(row.latest?.week_models ?? []);
  const weekPct = week?.pct ?? 0;
  const stroke = metricColor(weekPct);

  let style: CSSProperties = {};
  let lifted = false, parked = false;
  if (drag !== null) {
    if (drag.index === index) { lifted = true; style = { transform: `translateY(${drag.dy}px) scale(1.012)` }; }
    else { parked = true; style = { transform: `translateY(${rowShift(index, drag.index, drag.target, rowH)}px)` }; }
  }

  const cell = (key: ColumnKey): JSX.Element => {
    switch (key) {
      case "account": return (
        <div className="acct">
          <span className="acct-dot" style={{ background: accountDotColor(row, pill) }} />
          <span className={`acct-name${row.account.enabled ? "" : " acct-name-off"}`} title={row.account.config_dir}>{row.account.label}</span>
          {row.account.is_default && <span className="tag">default</span>}
          {pill.tone !== "success" && <StatusPill pill={pill} onShowFailure={onShowFailure} />}
        </div>);
      case "session": return <Meter pct={session?.pct ?? null} note={sessionNote(session, now)} />;
      case "week": { const n = weekNote(weekPct); return <Meter pct={week?.pct ?? null} note={week === null ? "no data" : n.text} noteWarn={n.warn} />; }
      case "model": return models === null ? <Meter pct={null} note="no data" /> : <Meter pct={models.pct} note={models.note} title={models.title} />;
      case "spark": return (
        <button type="button" className={`spark${chartOpen ? " spark-open" : ""}`} title="Click for 30 days" onClick={onToggleChart}>
          <Sparkline points={last7(points, now)} stroke={stroke} />
        </button>);
      case "updated": return <span className="updated">{formatAgo(row.latest?.taken_at ?? null, now)}</span>;
    }
  };

  const onGripKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>): void => {
    if (e.key === "ArrowUp") { e.preventDefault(); onMove(-1); }
    else if (e.key === "ArrowDown") { e.preventDefault(); onMove(1); }
  };

  return (
    <div className={["row", lifted ? "row-lifted" : "", parked ? "row-parked" : ""].filter(Boolean).join(" ")} style={style}>
      <div className="row-grid" style={{ gridTemplateColumns: gridCols }}>
        <div className="grip" role="button" tabIndex={0} aria-label="Drag to reorder" title="Drag to reorder rows" onPointerDown={onHandleDown} onKeyDown={onGripKeyDown}>
          <div className="grip-row"><div className="grip-dot" /><div className="grip-dot" /></div>
          <div className="grip-row"><div className="grip-dot" /><div className="grip-dot" /></div>
          <div className="grip-row"><div className="grip-dot" /><div className="grip-dot" /></div>
        </div>
        {columnOrder.map((key, ci) => (
          <div key={key} className={`cell${hotColumn === ci ? " cell-hot" : ""}`}>{cell(key)}</div>
        ))}
        <div className="row-actions">
          <button type="button" className={`btn btn-sm${editing ? " btn-edit-on" : ""}`} onClick={onToggleEdit}>edit</button>
        </div>
      </div>
      {chartOpen && <HistoryDrawer points={points} now={now} stroke={stroke} onCollapse={onToggleChart} />}
      {editing && (
        <EditDrawer
          row={row}
          index={index}
          total={total}
          now={now}
          onClose={onToggleEdit}
          onMove={onMove}
          onChanged={onChanged}
          onError={onError}
        />
      )}
    </div>
  );
}
