import type { AccountRow, Outcome } from "./types";

export type PillKind = "disabled" | "backoff" | "outcome" | "pending";

/**
 * How loudly the pill should read. Kept separate from `kind` because the
 * outcome pill alone spans the whole range: `ok` is the quiet, healthy state,
 * a guard trip is the one failure that has stopped polling altogether, and
 * everything between is a recoverable fault.
 */
export type PillTone = "success" | "warn" | "error" | "neutral";

/** Only a guard trip halts the poller, so only it reads as an error. */
const OUTCOME_TONES: Record<Outcome, PillTone> = {
  ok: "success",
  no_usage_data: "warn",
  parse_error: "warn",
  spawn_error: "warn",
  timeout: "warn",
  guard_tripped: "error",
};

export interface Pill {
  kind: PillKind;
  tone: PillTone;
  label: string;
  tooltip?: string;
  /** Only set for an outcome pill, so the row can open the failure detail. */
  outcome?: Outcome;
  snapshotId?: number;
}

const OUTCOME_LABELS: Record<Outcome, string> = {
  ok: "ok",
  no_usage_data: "no data — log in?",
  parse_error: "parse error",
  spawn_error: "spawn error",
  timeout: "timeout",
  guard_tripped: "guard tripped",
};

/**
 * Precedence, highest first: disabled, then backing off, then the latest
 * outcome. Purely presentational: every input comes from the backend.
 */
export function statusPill(row: AccountRow, now: number): Pill {
  if (!row.account.enabled) {
    const tooltip =
      row.account.disabled_reason === "guard_tripped"
        ? "disabled because the envelope guard tripped"
        : "disabled by you";
    return { kind: "disabled", tone: "neutral", label: "disabled", tooltip };
  }

  if (row.backoff_until !== null && row.backoff_until > now) {
    const minutes = Math.max(1, Math.ceil((row.backoff_until - now) / 60_000));
    return {
      kind: "backoff",
      tone: "warn",
      label: `backing off (next in ${minutes} min)`,
    };
  }

  if (row.latest === null) {
    return { kind: "pending", tone: "neutral", label: "not polled yet" };
  }

  return {
    kind: "outcome",
    tone: OUTCOME_TONES[row.latest.outcome],
    label: OUTCOME_LABELS[row.latest.outcome],
    tooltip: row.latest.error ?? undefined,
    outcome: row.latest.outcome,
    snapshotId: row.latest.id,
  };
}
