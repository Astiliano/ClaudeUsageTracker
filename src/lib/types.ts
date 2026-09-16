export type Outcome =
  | "ok"
  | "no_usage_data"
  | "parse_error"
  | "spawn_error"
  | "timeout"
  | "guard_tripped";

export type DisabledReason = "user" | "guard_tripped";
export type BinarySource = "override" | "local_bin" | "path";
export type Gate = "idle" | "active";

export interface Win {
  pct: number;
  resets_at: number | null;
}

export interface ModelWindow {
  label: string;
  pct: number;
  resets_at: number | null;
}

export interface SnapshotDto {
  id: number;
  account_id: string;
  taken_at: number;
  outcome: Outcome;
  session: Win | null;
  week_all: Win | null;
  week_models: ModelWindow[];
  error: string | null;
  duration_ms: number;
}

export interface Account {
  id: string;
  label: string;
  config_dir: string;
  enabled: boolean;
  disabled_reason: DisabledReason | null;
  is_default: boolean;
  created_at: number;
}

export interface AccountRow {
  account: Account;
  latest: SnapshotDto | null;
  backoff_until: number | null;
}

export interface BinaryInfo {
  path: string | null;
  source: BinarySource | null;
}

export interface Dashboard {
  accounts: AccountRow[];
  gate: Gate;
  busy: boolean;
  halted: string | null;
  stalled_at: number | null;
  binary: BinaryInfo;
  interval_secs: number;
}

export interface HistoryPoint {
  t: number;
  pct: number;
}

export interface UserSettings {
  interval_secs: number;
  timeout_secs: number;
  claude_binary: string;
  close_to_tray: boolean;
  launch_at_login: boolean;
  log_level: "info" | "debug";
}

export interface RawSnapshot {
  raw: string | null;
  error: string | null;
}

export interface AppErrorShape {
  code: string;
  message: string;
}

/** Narrowing guard so a caught `unknown` never needs an `any` cast. */
export function isAppError(e: unknown): e is AppErrorShape {
  return (
    typeof e === "object" &&
    e !== null &&
    typeof (e as { code?: unknown }).code === "string" &&
    typeof (e as { message?: unknown }).message === "string"
  );
}
