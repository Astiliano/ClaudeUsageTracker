import type { Backend } from "./backend";
import type {
  Account,
  AppErrorShape,
  Dashboard,
  HistoryPoint,
  RawSnapshot,
  SnapshotDto,
  UserSettings,
} from "./types";

const DAY_MS = 24 * 60 * 60 * 1000;
const MAX_HISTORY_DAYS = 30;
const DEFAULT_HISTORY_DAYS = 7;
const MIN_INTERVAL_SECS = 10;
const MAX_INTERVAL_SECS = 3600;
const MIN_TIMEOUT_SECS = 5;
const MAX_TIMEOUT_SECS = 120;

/** Commands that mutate the mock's in-memory state; every one is logged. */
const MUTATING_COMMANDS = new Set([
  "poll_now",
  "add_account",
  "update_account",
  "remove_account",
  "reorder_accounts",
  "set_settings",
  "clear_halt",
]);

interface MockAccount {
  account: Account;
  latest: SnapshotDto | null;
  history: HistoryPoint[];
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null;
}

function isUserSettings(v: unknown): v is UserSettings {
  return (
    isRecord(v) &&
    typeof v.interval_secs === "number" &&
    typeof v.timeout_secs === "number" &&
    typeof v.claude_binary === "string" &&
    typeof v.close_to_tray === "boolean" &&
    typeof v.launch_at_login === "boolean" &&
    (v.log_level === "info" || v.log_level === "debug")
  );
}

function stringArg(args: Record<string, unknown>, key: string): string {
  const v = args[key];
  if (typeof v !== "string") {
    throw { code: "internal", message: `mock: ${key} must be a string` } satisfies AppErrorShape;
  }
  return v;
}

function optionalStringArg(args: Record<string, unknown>, key: string): string | undefined {
  const v = args[key];
  return typeof v === "string" ? v : undefined;
}

function optionalNumberArg(args: Record<string, unknown>, key: string): number | undefined {
  const v = args[key];
  return typeof v === "number" ? v : undefined;
}

function optionalBooleanArg(args: Record<string, unknown>, key: string): boolean | undefined {
  const v = args[key];
  return typeof v === "boolean" ? v : undefined;
}

function numberArg(args: Record<string, unknown>, key: string): number {
  const v = args[key];
  if (typeof v !== "number") {
    throw { code: "internal", message: `mock: ${key} must be a number` } satisfies AppErrorShape;
  }
  return v;
}

function stringArrayArg(args: Record<string, unknown>, key: string): string[] {
  const v = args[key];
  if (!Array.isArray(v)) {
    throw { code: "internal", message: `mock: ${key} must be an array` } satisfies AppErrorShape;
  }
  const out: string[] = [];
  for (const item of v) {
    if (typeof item !== "string") {
      throw { code: "internal", message: `mock: ${key} must be a string array` } satisfies AppErrorShape;
    }
    out.push(item);
  }
  return out;
}

function clampDays(requested: number | undefined): number {
  const days = requested ?? DEFAULT_HISTORY_DAYS;
  return Math.min(MAX_HISTORY_DAYS, Math.max(1, Math.trunc(days)));
}

/**
 * Day index `29` is today. Each non-null day value seeds three hourly
 * samples (10:00, 13:00, 16:00 local) at `v-8`, `v`, `v-3`, clamped to >= 0.
 */
function buildHistory(days: ReadonlyArray<number | null>): HistoryPoint[] {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const points: HistoryPoint[] = [];
  days.forEach((v, i) => {
    if (v === null) return;
    const date = new Date(today);
    date.setDate(date.getDate() - (days.length - 1 - i));
    const sample = (hour: number, delta: number): void => {
      const at = new Date(date);
      at.setHours(hour, 0, 0, 0);
      points.push({ t: at.getTime(), pct: Math.max(0, v + delta) });
    };
    sample(10, -8);
    sample(13, 0);
    sample(16, -3);
  });
  return points;
}

function makeAccount(id: string, isDefault: boolean, sortOrder: number, now: number): Account {
  return {
    id,
    label: id,
    config_dir: `C:\\Users\\josh\\.${id}`,
    enabled: true,
    disabled_reason: null,
    is_default: isDefault,
    created_at: now - 30 * DAY_MS,
    sort_order: sortOrder,
  };
}

/** Translated from the design kit's `SAMPLE_ACCOUNTS` (docs/design/usage-tracker-kit.js). */
function seedAccounts(): MockAccount[] {
  const now = Date.now();
  const claude3Days = [
    12, 30, 18, 25, 44, 60, 38, 22, null, null, 55, 62, 48, 33, 27, 19, 36, 52, 64, 58, 40, 24, 16,
    28, 22, 41, 18, 60, 35, 46,
  ];
  const claudeDays = [
    48, 62, 71, 66, 80, 92, 88, 74, 69, 83, 95, 100, 97, 85, 78, 90, 99, 100, 94, 88, 76, 82, 91,
    100, 80, 95, 70, 100, 100, 100,
  ];
  const claude2Days = [
    30, 44, 52, 61, 58, 66, 74, 70, 63, 77, 85, 80, 72, null, 79, 88, 92, 86, 75, 81, 90, 96, 93,
    87, 55, 72, 88, 96, 90, 98,
  ];

  return [
    {
      account: makeAccount("claude3", true, 0, now),
      latest: {
        id: 1,
        account_id: "claude3",
        taken_at: now - 59_000,
        outcome: "ok",
        session: { pct: 40, resets_at: now + 2 * 60 * 60 * 1000 + 3 * 60 * 1000 },
        week_all: { pct: 46, resets_at: null },
        week_models: [{ label: "Fable", pct: 47, resets_at: null }],
        error: null,
        duration_ms: 1_200,
      },
      history: buildHistory(claude3Days),
    },
    {
      account: makeAccount("claude", false, 1, now),
      latest: {
        id: 2,
        account_id: "claude",
        taken_at: now - 56_000,
        outcome: "ok",
        session: { pct: 0, resets_at: null },
        week_all: { pct: 100, resets_at: null },
        week_models: [{ label: "Fable", pct: 68, resets_at: null }],
        error: null,
        duration_ms: 1_400,
      },
      history: buildHistory(claudeDays),
    },
    {
      account: makeAccount("claude2", false, 2, now),
      latest: {
        id: 3,
        account_id: "claude2",
        taken_at: now - 53_000,
        outcome: "timeout",
        session: { pct: 0, resets_at: null },
        week_all: { pct: 98, resets_at: null },
        week_models: [
          { label: "Fable", pct: 99, resets_at: null },
          { label: "Opus", pct: 12, resets_at: null },
        ],
        error: "claude exited after 30 s",
        duration_ms: 30_000,
      },
      history: buildHistory(claude2Days),
    },
  ];
}

/**
 * In-memory `Backend` seeded with the design's sample accounts, translated
 * into the real DTO shapes, so the UI can be exercised in a plain browser
 * (`VITE_MOCK_BACKEND=1`) without a running Tauri host.
 */
export function createMockBackend(): Backend {
  const accounts = seedAccounts();
  let settings: UserSettings = {
    interval_secs: 60,
    timeout_secs: 30,
    claude_binary: "",
    close_to_tray: true,
    launch_at_login: false,
    log_level: "info",
  };
  let nextAccountSeq = accounts.length + 1;

  const findAccount = (id: string): MockAccount => {
    const found = accounts.find((a) => a.account.id === id);
    if (found === undefined) {
      throw { code: "not_found", message: `no such account: ${id}` } satisfies AppErrorShape;
    }
    return found;
  };

  const dashboard = (): Dashboard => ({
    accounts: accounts.map((a) => ({
      account: a.account,
      latest: a.latest,
      backoff_until: null,
    })),
    gate: "active",
    busy: false,
    halted: null,
    stalled_at: null,
    binary: { path: "C:\\Users\\isjav\\.local\\bin\\claude.exe", source: "local_bin" },
    interval_secs: settings.interval_secs,
  });

  const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
    get_dashboard: () => dashboard(),

    get_history: (args) => {
      const accountId = stringArg(args, "accountId");
      const days = clampDays(optionalNumberArg(args, "days"));
      const since = Date.now() - days * DAY_MS;
      return findAccount(accountId).history.filter((p) => p.t >= since);
    },

    poll_now: () => {
      const now = Date.now();
      for (const a of accounts) {
        if (a.latest !== null) {
          a.latest = { ...a.latest, taken_at: now };
        }
      }
      return "started";
    },

    add_account: (args) => {
      const configDir = stringArg(args, "configDir");
      if (accounts.some((a) => a.account.config_dir === configDir)) {
        throw {
          code: "duplicate",
          message: `an account already exists for ${configDir}`,
        } satisfies AppErrorShape;
      }
      const segments = configDir.split(/[\\/]/).filter((s) => s.length > 0);
      const label = segments[segments.length - 1] ?? configDir;
      const id = `mock-${nextAccountSeq}`;
      nextAccountSeq += 1;
      const account: Account = {
        id,
        label,
        config_dir: configDir,
        enabled: false,
        disabled_reason: "user",
        is_default: false,
        created_at: Date.now(),
        sort_order: accounts.length,
      };
      accounts.push({ account, latest: null, history: [] });
      return account;
    },

    update_account: (args) => {
      const id = stringArg(args, "id");
      const label = optionalStringArg(args, "label");
      const enabled = optionalBooleanArg(args, "enabled");
      const found = findAccount(id);
      if (label !== undefined) {
        found.account = { ...found.account, label };
      }
      if (enabled !== undefined) {
        found.account = {
          ...found.account,
          enabled,
          disabled_reason: enabled ? null : "user",
        };
      }
      return found.account;
    },

    remove_account: (args) => {
      const id = stringArg(args, "id");
      const idx = accounts.findIndex((a) => a.account.id === id);
      if (idx === -1) {
        throw { code: "not_found", message: `no such account: ${id}` } satisfies AppErrorShape;
      }
      accounts.splice(idx, 1);
      return undefined;
    },

    reorder_accounts: (args) => {
      const ids = stringArrayArg(args, "ids");
      const reordered = ids.map((id) => findAccount(id));
      reordered.forEach((a, index) => {
        a.account = { ...a.account, sort_order: index };
      });
      accounts.length = 0;
      accounts.push(...reordered);
      return accounts.map((a) => a.account);
    },

    rescan_profiles: () => [],

    get_settings: () => settings,

    set_settings: (args) => {
      const next = args.settings;
      if (!isUserSettings(next)) {
        throw {
          code: "internal",
          message: "mock: settings must be a UserSettings object",
        } satisfies AppErrorShape;
      }
      if (next.interval_secs < MIN_INTERVAL_SECS || next.interval_secs > MAX_INTERVAL_SECS) {
        throw { code: "out_of_range", message: "interval_secs must be 10..=3600" } satisfies AppErrorShape;
      }
      if (next.timeout_secs < MIN_TIMEOUT_SECS || next.timeout_secs > MAX_TIMEOUT_SECS) {
        throw { code: "out_of_range", message: "timeout_secs must be 5..=120" } satisfies AppErrorShape;
      }
      settings = next;
      return undefined;
    },

    clear_halt: () => undefined,

    open_login: (args) => {
      findAccount(stringArg(args, "id"));
      return undefined;
    },

    open_log_dir: () => undefined,

    get_snapshot_raw: (args) => {
      const snapshotId = numberArg(args, "snapshotId");
      const found = accounts.find((a) => a.latest?.id === snapshotId);
      return {
        raw: "Session: 40% …",
        error: found?.latest?.error ?? null,
      } satisfies RawSnapshot;
    },
  };

  return {
    invoke: async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
      const resolvedArgs = args ?? {};
      const handler = handlers[command];
      if (handler === undefined) {
        throw {
          code: "internal",
          message: `mock: unknown command ${command}`,
        } satisfies AppErrorShape;
      }
      if (MUTATING_COMMANDS.has(command)) {
        console.debug("mock", command, resolvedArgs);
      }
      const result = handler(resolvedArgs);
      // The mock's one unsafe boundary: handlers return the DTO shape the
      // command contract promises, but the map itself is typed `unknown` so
      // this cast is required to hand the caller back a `T`.
      return result as T;
    },
    listen: () => Promise.resolve(() => undefined),
  };
}
