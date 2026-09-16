import type { JSX, PointerEvent as ReactPointerEvent } from "react";
import { useEffect, useRef, useState } from "react";
import { backend } from "../lib/backend";
import { COLUMNS, type ColumnKey, gridTemplate } from "../lib/columns";
import { type Rect, colDragTarget, colLineX, rowDragTarget } from "../lib/drag";
import { moveItem } from "../lib/reorder";
import type { AccountRow as AccountRowData, HistoryPoint } from "../lib/types";
import { AccountRow } from "./AccountRow";

export interface RowDragState { index: number; target: number; startY: number; dy: number }
export interface ColDragState { index: number; target: number; startX: number; dx: number; lineX: number }

interface Props {
  rows: AccountRowData[];
  history: Record<string, HistoryPoint[]>;
  now: number;
  columnOrder: ColumnKey[];
  onColumnOrder: (order: ColumnKey[]) => void;
  onChanged: () => void;
  onError: (message: string) => void;
  onShowFailure: (snapshotId: number) => void;
}

const DEFAULT_ROW_H = 66;

function beginBodyDrag(): void {
  document.body.style.cursor = "grabbing";
  document.body.style.userSelect = "none";
}
function endBodyDrag(): void {
  document.body.style.cursor = "";
  document.body.style.userSelect = "";
}

export function AccountsTable({ rows, history, now, columnOrder, onColumnOrder, onChanged, onError, onShowFailure }: Props): JSX.Element {
  const [order, setOrder] = useState<AccountRowData[]>(rows);
  const [drag, setDrag] = useState<RowDragState | null>(null);
  const [colDrag, setColDrag] = useState<ColDragState | null>(null);
  const [chartId, setChartId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);

  // Refs mirror the values the stable window handlers need to read.
  const orderRef = useRef(order);
  const columnOrderRef = useRef(columnOrder);
  const dragRef = useRef<RowDragState | null>(null);
  const colDragRef = useRef<ColDragState | null>(null);
  const rowH = useRef(DEFAULT_ROW_H);
  const colRects = useRef<Rect[]>([]);
  const wrapLeft = useRef(0);
  const onColumnOrderRef = useRef(onColumnOrder);

  useEffect(() => { setOrder(rows); }, [rows]);
  useEffect(() => { orderRef.current = order; }, [order]);
  useEffect(() => { columnOrderRef.current = columnOrder; }, [columnOrder]);
  useEffect(() => { onColumnOrderRef.current = onColumnOrder; }, [onColumnOrder]);

  const commitOrder = async (next: AccountRowData[]): Promise<void> => {
    setOrder(next);
    try {
      await backend().invoke("reorder_accounts", { ids: next.map((r) => r.account.id) });
      onChanged();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
      onChanged(); // optimistic order may not match what was persisted; resync
    }
  };
  const commitOrderRef = useRef(commitOrder);
  useEffect(() => { commitOrderRef.current = commitOrder; });

  /* ---- stable window handlers (created once) ---- */
  const handlers = useRef({
    rowMove: (e: PointerEvent): void => {
      const d = dragRef.current;
      if (d === null) return;
      const dy = e.clientY - d.startY;
      const target = rowDragTarget(d.index, dy, rowH.current, orderRef.current.length);
      dragRef.current = { ...d, dy, target };
      setDrag(dragRef.current);
    },
    rowUp: (): void => {
      const d = dragRef.current;
      detachRow();
      dragRef.current = null;
      setDrag(null);
      if (d !== null && d.target !== d.index) {
        void commitOrderRef.current(moveItem(orderRef.current, d.index, d.target));
      }
    },
    colMove: (e: PointerEvent): void => {
      const c = colDragRef.current;
      if (c === null) return;
      const target = colDragTarget(colRects.current, e.clientX);
      colDragRef.current = { ...c, dx: e.clientX - c.startX, target, lineX: colLineX(colRects.current, target, c.index, wrapLeft.current) };
      setColDrag(colDragRef.current);
    },
    colUp: (): void => {
      const c = colDragRef.current;
      detachCol();
      colDragRef.current = null;
      setColDrag(null);
      if (c !== null && c.target !== c.index) {
        onColumnOrderRef.current(moveItem(columnOrderRef.current, c.index, c.target));
      }
    },
  });

  function detachRow(): void {
    window.removeEventListener("pointermove", handlers.current.rowMove);
    window.removeEventListener("pointerup", handlers.current.rowUp);
    endBodyDrag();
  }
  function detachCol(): void {
    window.removeEventListener("pointermove", handlers.current.colMove);
    window.removeEventListener("pointerup", handlers.current.colUp);
    endBodyDrag();
  }
  useEffect(() => () => { detachRow(); detachCol(); }, []);

  const startRowDrag = (e: ReactPointerEvent<HTMLDivElement>, index: number): void => {
    e.preventDefault();
    const grid = e.currentTarget.parentElement;
    rowH.current = grid !== null ? grid.offsetHeight + 1 : DEFAULT_ROW_H;
    beginBodyDrag();
    setChartId(null);
    setEditingId(null);
    dragRef.current = { index, target: index, startY: e.clientY, dy: 0 };
    setDrag(dragRef.current);
    window.addEventListener("pointermove", handlers.current.rowMove);
    window.addEventListener("pointerup", handlers.current.rowUp);
  };

  const startColDrag = (e: ReactPointerEvent<HTMLDivElement>, index: number): void => {
    e.preventDefault();
    const head = e.currentTarget.parentElement;
    if (head === null) return;
    colRects.current = Array.from(head.querySelectorAll<HTMLElement>("[data-colcell]")).map((el) => {
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, width: r.width };
    });
    wrapLeft.current = (head.parentElement ?? head).getBoundingClientRect().left;
    beginBodyDrag();
    setChartId(null);
    setEditingId(null);
    colDragRef.current = { index, target: index, startX: e.clientX, dx: 0, lineX: colLineX(colRects.current, index, index, wrapLeft.current) };
    setColDrag(colDragRef.current);
    window.addEventListener("pointermove", handlers.current.colMove);
    window.addEventListener("pointerup", handlers.current.colUp);
  };

  const moveBy = (index: number, delta: number): void => {
    const target = index + delta;
    if (target < 0 || target >= order.length) return;
    void commitOrder(moveItem(order, index, target));
  };

  const gridCols = gridTemplate(columnOrder);
  return (
    <section className="panel" aria-label="Accounts">
      <div className="thead-wrap">
        <div className="thead" style={{ gridTemplateColumns: gridCols }}>
          <div />
          {columnOrder.map((key, ci) => {
            const dragging = colDrag !== null && colDrag.index === ci;
            const cls = ["th", dragging ? "th-dragging" : "", colDrag !== null && !dragging ? "th-dimmed" : ""].filter(Boolean).join(" ");
            return (
              <div key={key} data-colcell="1" className={cls} title="Drag to move column"
                style={{ transform: dragging ? `translateX(${colDrag.dx}px)` : undefined }}
                onPointerDown={(e) => startColDrag(e, ci)}>
                {COLUMNS[key].label}
              </div>
            );
          })}
          <div />
        </div>
        {colDrag !== null && <div className="col-line" style={{ left: `${colDrag.lineX}px` }} />}
      </div>
      <div className="rows">
        {drag !== null && (
          <div className="row-placeholder" style={{ top: `${drag.target * rowH.current + 6}px`, height: `${rowH.current - 12}px` }} />
        )}
        {order.map((row, idx) => (
          <AccountRow key={row.account.id} row={row} index={idx} total={order.length}
            points={history[row.account.id] ?? []} now={now} columnOrder={columnOrder} gridCols={gridCols}
            hotColumn={colDrag?.index ?? null} drag={drag} rowH={rowH.current}
            chartOpen={chartId === row.account.id} editing={editingId === row.account.id}
            onHandleDown={(e) => startRowDrag(e, idx)}
            onToggleChart={() => { setEditingId(null); setChartId((c) => (c === row.account.id ? null : row.account.id)); }}
            onToggleEdit={() => { setChartId(null); setEditingId((c) => (c === row.account.id ? null : row.account.id)); }}
            onMove={(delta) => moveBy(idx, delta)}
            onChanged={onChanged} onError={onError} onShowFailure={onShowFailure} />
        ))}
      </div>
    </section>
  );
}
