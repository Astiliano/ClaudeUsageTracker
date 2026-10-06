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
  created_at: number;
  sort_order: number;
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
  /** Set while automatic refreshes are held for low commit headroom. */
  memory_hold: MemoryHold | null;
}

/** Mirrors the Rust `MemoryHold`: bytes at the last held decision, and the epoch-ms it began. */
export interface MemoryHold {
  available_bytes: number;
  floor_bytes: number;
  since: number;
}

export interface SystemStats {
  sampled_at: number;
  /** Whole-machine CPU busy share, 0..100. */
  cpu_pct: number;
  mem_used_bytes: number;
  mem_total_bytes: number;
  claude_count: number;
}

export interface SystemReport {
  stats: SystemStats | null;
  stopped: boolean;
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
  /** Automatic refreshes hold below this much free commit, in MB; 0 never holds. */
  min_free_memory_mb: number;
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
