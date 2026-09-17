import type { Backend } from "./backend";
import type { Metric } from "./history";
import limits from "./historyLimits.json";
import type {
  Account,
  AppErrorShape,
  Dashboard,
  HistoryPoint,
  RawSnapshot,
  SnapshotDto,
  SystemReport,
  SystemStats,
  UserSettings,
} from "./types";

const DAY_MS = 24 * 60 * 60 * 1000;
const MIN_INTERVAL_SECS = 10;
const MAX_INTERVAL_SECS = 3600;
const MIN_TIMEOUT_SECS = 5;
const MAX_TIMEOUT_SECS = 120;
const RANGE_SLACK_MS = 300_000;
const MAX_LABEL_LEN = 64;

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

/** One poll, as the real `snapshots` row stores it (minus the noise). */
interface Sample { t: number; session: number; week_all: number; models: Array<{ label: string; pct: number }> }

interface MockAccount {
  account: Account;
  latest: SnapshotDto | null;
  samples: Sample[];
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

const HISTORY_START_HOUR = 9;
const HISTORY_PEAK_HOUR = 14;
const HISTORY_END_HOUR = 18;
const SAMPLE_GAP_MS = 2 * 60_000;

function clampPct(v: number): number {
  return Math.max(0, Math.min(100, Math.round(v)));
}

/**
 * Day index `29` is today. Each non-null day value seeds one sample every
 * two minutes from 09:00 to 18:00 local (roughly real poll density), so
 * every preset/unit combination has something to show. Week-all rises from
 * `v-8` at 09:00 to `v` at 14:00 then eases down to `v-3`; session is a
 * half-sine over the working day; each model sits at `v + offset`.
 */
function buildSamples(days: ReadonlyArray<number | null>, models: ReadonlyArray<{ label: string; offset: number }>): Sample[] {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const out: Sample[] = [];
  days.forEach((v, i) => {
    if (v === null) return;
    const date = new Date(today);
    date.setDate(date.getDate() - (days.length - 1 - i));
    const start = new Date(date);
    start.setHours(HISTORY_START_HOUR, 0, 0, 0);
    const end = new Date(date);
    end.setHours(HISTORY_END_HOUR, 0, 0, 0);
    const peak = new Date(date);
    peak.setHours(HISTORY_PEAK_HOUR, 0, 0, 0);
    for (let t = start.getTime(); t <= end.getTime(); t += SAMPLE_GAP_MS) {
      let delta: number;
      if (t <= peak.getTime()) {
        const f = (t - start.getTime()) / (peak.getTime() - start.getTime());
        delta = -8 + 8 * f;
      } else {
        const f = (t - peak.getTime()) / (end.getTime() - peak.getTime());
        delta = -3 * f * f;
      }
      const dayFraction = (t - start.getTime()) / (end.getTime() - start.getTime());
      out.push({
        t,
        week_all: clampPct(v + delta),
        session: clampPct(70 * Math.sin(Math.PI * dayFraction)),
        models: models.map((m) => ({ label: m.label, pct: clampPct(v + delta + m.offset) })),
      });
    }
  });
  return out;
}

function makeAccount(id: string, sortOrder: number, now: number): Account {
  return {
    id,
    label: id,
    config_dir: `C:\\Users\\josh\\.${id}`,
    enabled: true,
    disabled_reason: null,
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
      account: makeAccount("claude3", 0, now),
      latest: {
        id: 1,
        account_id: "claude3",
        taken_at: now - 59_000,
        outcome: "ok",
        session: { pct: 40, resets_at: now + 2 * 60 * 60 * 1000 + 3 * 60 * 1000 },
        week_all: { pct: 46, resets_at: now + 2 * 24 * 60 * 60 * 1000 + 3 * 60 * 60 * 1000 },
        week_models: [{ label: "Fable", pct: 47, resets_at: null }],
        error: null,
        duration_ms: 1_200,
      },
      samples: buildSamples(claude3Days, [{ label: "Fable", offset: 1 }, { label: "Opus", offset: -20 }]),
    },
    {
      account: makeAccount("claude", 1, now),
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
      samples: buildSamples(claudeDays, [{ label: "Fable", offset: -30 }]),
    },
    {
      account: makeAccount("claude2", 2, now),
      latest: {
        id: 3,
        account_id: "claude2",
        taken_at: now - 53_000,
        outcome: "timeout",
        session: { pct: 0, resets_at: null },
        week_all: { pct: 98, resets_at: now + 5 * 24 * 60 * 60 * 1000 + 14 * 60 * 60 * 1000 },
        week_models: [
          { label: "Fable", pct: 99, resets_at: null },
          { label: "Opus", pct: 12, resets_at: null },
        ],
        error: "claude exited after 30 s",
        duration_ms: 30_000,
      },
      samples: buildSamples(claude2Days, [{ label: "Fable", offset: 1 }, { label: "Sonnet", offset: -40 }]),
    },
  ];
}

function isMetric(v: unknown): v is Metric {
  if (!isRecord(v)) return false;
  if (v.kind === "week_all" || v.kind === "session") return true;
  return v.kind === "model" && typeof v.label === "string";
}

function metricArg(args: Record<string, unknown>): Metric {
  const v = args.metric;
  if (!isMetric(v)) {
    throw { code: "internal", message: "mock: metric must be a HistoryMetric" } satisfies AppErrorShape;
  }
  return v;
}

function outOfRange(message: string): AppErrorShape {
  return { code: "out_of_range", message };
}

/** Mirrors commands::validate_history_request so the UI's error path is exercisable in a browser. */
function validateHistory(now: number, since: number, bucketMs: number, metric: Metric): void {
  if (bucketMs < limits.minBucketMs) throw outOfRange(`bucket_ms must be >= ${limits.minBucketMs}, got ${bucketMs}`);
  if (since > now) throw outOfRange("since must not be in the future");
  const range = now - since;
  if (range > limits.maxRangeMs + RANGE_SLACK_MS) throw outOfRange(`range must be <= ${limits.maxRangeMs} ms`);
  if (Math.ceil(range / bucketMs) > limits.maxBuckets) throw outOfRange(`request spans more than ${limits.maxBuckets} buckets`);
  if (metric.kind === "model") {
    const trimmed = metric.label.trim();
    if (trimmed.length === 0) throw outOfRange("model label must not be blank");
    if ([...trimmed].length > MAX_LABEL_LEN) throw outOfRange(`model label must be <= ${MAX_LABEL_LEN} characters`);
  }
}

function sampleValue(s: Sample, metric: Metric): number | null {
  switch (metric.kind) {
    case "week_all": return s.week_all;
    case "session": return s.session;
    case "model": return s.models.find((m) => m.label === metric.label)?.pct ?? null;
  }
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

  // The only URL-driven behaviour in the mock, so Playwright can reach the
  // dimmed and unavailable states without a rebuild. The real backend never
  // looks at the URL.
  const mockSystem = new URLSearchParams(window.location.search).get("mockSystem");
  const GIB = 1024 * 1024 * 1024;

  const systemStats = (): SystemStats => ({
    sampled_at: Date.now(),
    cpu_pct: Math.round((5 + Math.random() * 25) * 10) / 10,
    mem_used_bytes: Math.round(13 * GIB * (0.97 + Math.random() * 0.06)),
    mem_total_bytes: 32 * GIB,
    claude_count: 2,
  });

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
      const since = numberArg(args, "since");
      const bucketMs = numberArg(args, "bucketMs");
      const metric = metricArg(args);
      validateHistory(Date.now(), since, bucketMs, metric);
      const buckets = new Map<number, number>();
      for (const s of findAccount(accountId).samples) {
        if (s.t < since) continue;
        const v = sampleValue(s, metric);
        if (v === null) continue;
        const b = since + Math.floor((s.t - since) / bucketMs) * bucketMs;
        buckets.set(b, Math.max(buckets.get(b) ?? 0, v));
      }
      return [...buckets.entries()]
        .sort((a, b) => a[0] - b[0])
        .map(([t, pct]): HistoryPoint => ({ t, pct }));
    },

    get_history_models: (args) => {
      const accountId = stringArg(args, "accountId");
      const labels = new Set<string>();
      for (const s of findAccount(accountId).samples) for (const m of s.models) labels.add(m.label);
      return [...labels].sort();
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
        created_at: Date.now(),
        sort_order: accounts.length,
      };
      accounts.push({ account, latest: null, samples: [] });
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

    get_system: (): SystemReport => {
      if (mockSystem === "error") {
        throw {
          code: "internal",
          message: "mock: sampler unreachable",
        } satisfies AppErrorShape;
      }
      return { stats: systemStats(), stopped: mockSystem === "stopped" };
    },

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
    listen: (event: string, handler: () => void) => {
      if (event !== "system:sampled") return Promise.resolve(() => undefined);
      const timer = window.setInterval(handler, 5000);
      return Promise.resolve(() => window.clearInterval(timer));
    },
    setAlwaysOnTop: async (flag) => { console.info("mock: setAlwaysOnTop", flag); },
  };
}
