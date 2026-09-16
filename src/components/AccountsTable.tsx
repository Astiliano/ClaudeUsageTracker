import { invoke } from "@tauri-apps/api/core";
import type { JSX } from "react";
import { useEffect, useState } from "react";
import { formatAgo, formatCountdown } from "../lib/format";
import { statusPill } from "../lib/pill";
import { moveItem } from "../lib/reorder";
import type { AccountRow, HistoryPoint } from "../lib/types";
import { Sparkline } from "./Sparkline";

interface Props {
  rows: AccountRow[];
  history: Record<string, HistoryPoint[]>;
  now: number;
  onChanged: () => void;
  onError: (message: string) => void;
  onShowFailure: (snapshotId: number) => void;
}

function Bar({ pct }: { pct: number }): JSX.Element {
  const tone = pct >= 90 ? "red" : pct >= 70 ? "amber" : "green";
  return (
    <div className="bar" title={`${pct}%`}>
      <div className={`bar-fill bar-${tone}`} style={{ width: `${pct}%` }} />
      <span className="bar-label">{pct}%</span>
    </div>
  );
}

export function AccountsTable({
  rows,
  history,
  now,
  onChanged,
  onError,
  onShowFailure,
}: Props): JSX.Element {
  const [renaming, setRenaming] = useState<string | null>(null);
  const [draftLabel, setDraftLabel] = useState<string>("");
  // Local, reorderable view of `rows`. Kept separate so a drag or a Move
  // up/down click can show its result immediately (optimistic reorder)
  // instead of waiting for the debounced dashboard refetch; re-synced from
  // `rows` whenever the backend's own order changes.
  const [order, setOrder] = useState<AccountRow[]>(rows);
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const [overIndex, setOverIndex] = useState<number | null>(null);

  useEffect(() => {
    setOrder(rows);
  }, [rows]);

  const call = async (
    command: string,
    args: Record<string, unknown>,
  ): Promise<void> => {
    try {
      await invoke(command, args);
      onChanged();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  const commitOrder = async (next: AccountRow[]): Promise<void> => {
    setOrder(next);
    try {
      await invoke("reorder_accounts", { ids: next.map((r) => r.account.id) });
      onChanged();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
      // The optimistic order may not match what got persisted; resync.
      onChanged();
    }
  };

  const handleDrop = (dropIndex: number): void => {
    const from = dragIndex;
    setDragIndex(null);
    setOverIndex(null);
    if (from === null || from === dropIndex) {
      return;
    }
    void commitOrder(moveItem(order, from, dropIndex));
  };

  const moveByKeyboard = (index: number, delta: number): void => {
    const target = index + delta;
    if (target < 0 || target >= order.length) {
      return;
    }
    void commitOrder(moveItem(order, index, target));
  };

  return (
    <table className="accounts">
      <thead>
        <tr>
          <th aria-hidden="true"></th>
          <th>Account</th>
          <th>Session</th>
          <th>Week (all)</th>
          <th>Per model</th>
          <th>Last 7 days</th>
          <th>Updated</th>
          <th>Status</th>
          <th>Actions</th>
        </tr>
      </thead>
      <tbody>
        {order.map((row, idx) => {
          const pill = statusPill(row, now);
          const session = row.latest?.session ?? null;
          const week = row.latest?.week_all ?? null;
          const models = row.latest?.week_models ?? [];
          const rowClasses = [
            row.account.enabled ? "" : "row-off",
            dragIndex === idx ? "row-dragging" : "",
            overIndex === idx && dragIndex !== null && dragIndex !== idx
              ? "row-drag-over"
              : "",
          ]
            .filter(Boolean)
            .join(" ");
          return (
            <tr
              key={row.account.id}
              className={rowClasses}
              draggable
              onDragStart={() => setDragIndex(idx)}
              onDragOver={(e) => {
                e.preventDefault();
                setOverIndex(idx);
              }}
              onDrop={(e) => {
                e.preventDefault();
                handleDrop(idx);
              }}
              onDragEnd={() => {
                setDragIndex(null);
                setOverIndex(null);
              }}
            >
              <td className="grip-cell">
                <span className="grip" aria-label="Drag to reorder" title="Drag to reorder">
                  ⋮⋮
                </span>
              </td>
              <td>
                {renaming === row.account.id ? (
                  <form
                    onSubmit={(e) => {
                      e.preventDefault();
                      setRenaming(null);
                      void call("update_account", {
                        id: row.account.id,
                        label: draftLabel,
                      });
                    }}
                  >
                    <input
                      value={draftLabel}
                      onChange={(e) => setDraftLabel(e.target.value)}
                      autoFocus
                    />
                  </form>
                ) : (
                  <span title={row.account.config_dir}>
                    {row.account.label}
                    {row.account.is_default && <em className="tag">default</em>}
                  </span>
                )}
              </td>
              <td>
                {session !== null ? (
                  <>
                    <Bar pct={session.pct} />
                    <div className="sub">
                      {formatCountdown(session.resets_at, now)}
                    </div>
                  </>
                ) : (
                  "—"
                )}
              </td>
              <td>{week !== null ? <Bar pct={week.pct} /> : "—"}</td>
              <td>
                {models.length === 0
                  ? "—"
                  : models.map((m) => (
                      <span key={m.label} className="model">
                        {m.label} {m.pct}%
                      </span>
                    ))}
              </td>
              <td>
                <Sparkline points={history[row.account.id] ?? []} />
              </td>
              <td>{formatAgo(row.latest?.taken_at ?? null, now)}</td>
              <td>
                <button
                  type="button"
                  className={`pill pill-${pill.kind} pill-tone-${pill.tone}`}
                  title={pill.tooltip}
                  disabled={pill.snapshotId === undefined || pill.outcome === "ok"}
                  onClick={() => {
                    if (pill.snapshotId !== undefined) {
                      onShowFailure(pill.snapshotId);
                    }
                  }}
                >
                  {pill.label}
                </button>
              </td>
              <td className="actions">
                <button
                  type="button"
                  className="move-btn"
                  aria-label="Move up"
                  title="Move up"
                  disabled={idx === 0}
                  onClick={() => moveByKeyboard(idx, -1)}
                >
                  ▲
                </button>
                <button
                  type="button"
                  className="move-btn"
                  aria-label="Move down"
                  title="Move down"
                  disabled={idx === order.length - 1}
                  onClick={() => moveByKeyboard(idx, 1)}
                >
                  ▼
                </button>
                <button
                  type="button"
                  onClick={() =>
                    void call("update_account", {
                      id: row.account.id,
                      enabled: !row.account.enabled,
                    })
                  }
                >
                  {row.account.enabled ? "Disable" : "Enable"}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setRenaming(row.account.id);
                    setDraftLabel(row.account.label);
                  }}
                >
                  Rename
                </button>
                <button
                  type="button"
                  onClick={() => void call("open_login", { id: row.account.id })}
                >
                  Log in
                </button>
                <button
                  type="button"
                  onClick={() => void call("remove_account", { id: row.account.id })}
                >
                  Remove
                </button>
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
