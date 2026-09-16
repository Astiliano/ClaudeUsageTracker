import type { JSX, KeyboardEvent } from "react";
import { useState } from "react";
import { backend } from "../lib/backend";
import { errorMessage } from "../lib/errors";
import { formatCountdown } from "../lib/format";
import type { AccountRow as AccountRowData } from "../lib/types";

interface Props {
  row: AccountRowData;
  index: number;
  total: number;
  now: number;
  onClose: () => void;
  onMove: (delta: number) => void;
  onChanged: () => void;
  onError: (m: string) => void;
}

export function EditDrawer({ row, index, total, now, onClose, onMove, onChanged, onError }: Props): JSX.Element {
  const { account } = row;
  const [draft, setDraft] = useState(account.label);

  const call = async (command: string, args: Record<string, unknown>): Promise<void> => {
    try {
      await backend().invoke(command, args);
    } catch (e) {
      onError(errorMessage(e));
    }
    onChanged();
  };

  const save = (): void => {
    void call("update_account", { id: account.id, label: draft.trim() || account.label });
    onClose();
  };

  const remove = (): void => {
    void call("remove_account", { id: account.id });
    onClose();
  };

  const onNameKeyDown = (e: KeyboardEvent<HTMLInputElement>): void => {
    if (e.key === "Enter") save();
    else if (e.key === "Escape") onClose();
  };

  const session = row.latest?.session ?? null;
  const week = row.latest?.week_all ?? null;
  const models = row.latest?.week_models ?? [];

  return (
    <div className="drawer">
      <div className="edit-row">
        <span className="edit-label">Name</span>
        <input
          className="input"
          value={draft}
          autoFocus
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onNameKeyDown}
        />
        <button type="button" className="btn btn-sm btn-primary" onClick={save}>save</button>
        <button type="button" className="btn btn-sm btn-ghost" onClick={onClose}>cancel</button>
      </div>

      <div className="edit-row">
        <span className="edit-label">Resets</span>
        <span className="edit-hint input-mono">
          session · {session === null ? "no data" : formatCountdown(session.resets_at, now)}
        </span>
        <span className="edit-hint input-mono">
          weekly window · {week === null ? "no data" : formatCountdown(week.resets_at, now)}
        </span>
        {models.map((m) => (
          <span key={m.label} className="edit-hint input-mono">
            {m.label} · {formatCountdown(m.resets_at, now)}
          </span>
        ))}
      </div>

      <div className="edit-row">
        <span className="edit-label">Path</span>
        <span className="edit-path" title={account.config_dir}>{account.config_dir}</span>
      </div>

      <div className="edit-row">
        <span className="edit-label">Account</span>
        <button
          type="button"
          className="btn btn-sm"
          onClick={() => void call("update_account", { id: account.id, enabled: !account.enabled })}
        >
          {account.enabled ? "disable" : "enable"}
        </button>
        <button type="button" className="btn btn-sm" onClick={() => void call("open_login", { id: account.id })}>
          log in
        </button>
        <button type="button" className="btn btn-sm" disabled={index === 0} onClick={() => onMove(-1)}>
          move up
        </button>
        <button type="button" className="btn btn-sm" disabled={index === total - 1} onClick={() => onMove(1)}>
          move down
        </button>
        <button type="button" className="btn btn-sm btn-danger ml-auto" onClick={remove}>
          remove
        </button>
      </div>
    </div>
  );
}
