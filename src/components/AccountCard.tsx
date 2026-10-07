import type { JSX } from "react";
import { statusPill } from "../lib/pill";
import { accountDotColor, summarizeModels } from "../lib/present";
import type { AccountRow as AccountRowData } from "../lib/types";
import { Ring } from "./Ring";
import { StatusPill } from "./StatusPill";

interface Props { row: AccountRowData; now: number; onShowFailure: (id: number) => void }

/** Compact layout: name, status, and three ring gauges. No drag, no drawers. */
export function AccountCard({ row, now, onShowFailure }: Props): JSX.Element {
  const pill = statusPill(row, now);
  const models = summarizeModels(row.latest?.week_models ?? []);
  return (
    <div className="card" role="listitem">
      <div className="acct">
        <span className="acct-dot" style={{ background: accountDotColor(row, pill) }} />
        <span className={`acct-name${row.account.enabled ? "" : " acct-name-off"}`} title={row.account.config_dir}>{row.account.label}</span>
        {pill.tone !== "success" && <StatusPill pill={pill} onShowFailure={onShowFailure} />}
      </div>
      <div className="card-rings">
        <Ring pct={row.latest?.session?.pct ?? null} label="session" />
        <Ring pct={row.latest?.week_all?.pct ?? null} label="week" />
        {models === null
          ? <Ring pct={null} label="model" />
          : <Ring pct={models.pct} label={models.label} title={models.title} />}
      </div>
    </div>
  );
}
