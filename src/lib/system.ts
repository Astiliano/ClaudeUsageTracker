import type { SystemReport, SystemStats } from "./types";

const MIB = 1024 * 1024;
const GIB = 1024 * MIB;

export const SAMPLE_INTERVAL_MS = 5000;
/** Three missed samples: the sampler has died, or the machine slept. */
export const STALE_AFTER_MS = 3 * SAMPLE_INTERVAL_MS;

const SAMPLER_STOPPED = "sampler stopped, see log";

/** Whole MiB below a GiB, one decimal at or above it. */
export function formatBytes(bytes: number): string {
  if (bytes < GIB) return `${Math.round(bytes / MIB)} MB`;
  return `${(bytes / GIB).toFixed(1)} GB`;
}

/** The machine's memory in use as a share of the total, or null without a total. */
export function memPct(stats: SystemStats): number | null {
  if (stats.mem_total_bytes === 0) return null;
  const pct = (stats.mem_used_bytes / stats.mem_total_bytes) * 100;
  return Math.max(0, Math.min(100, pct));
}

export function isStale(stats: SystemStats, now: number): boolean {
  return now - stats.sampled_at > STALE_AFTER_MS;
}

export interface SysItem {
  key: "cpu" | "mem" | "count" | "waiting" | "unavailable";
  pct: number | null;
  text: string;
  title: string;
}

export interface SysLine {
  items: SysItem[];
  dimmed: boolean;
}

export function processCountSuffix(count: number | null): string {
  if (count === null || count === 0) return "";
  return count === 1 ? " · 1 Claude process" : ` · ${count} Claude processes`;
}

/**
 * HH:MM:SS in the viewer's local zone. Written here rather than reusing
 * `clockOf` from banner.ts, which has no seconds, and not via
 * `toLocaleTimeString`, whose output depends on the host locale and would
 * make the test unpinnable.
 */
export function clock(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number): string => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function countText(count: number): string {
  return count === 1 ? "1 proc" : `${count} procs`;
}

function liveItems(stats: SystemStats, showCount: boolean): SysItem[] {
  const cpu = Math.round(stats.cpu_pct);
  const mem = memPct(stats);
  const items: SysItem[] = [
    { key: "cpu", pct: stats.cpu_pct, text: `cpu ${cpu}%`, title: `machine CPU: ${cpu}% busy` },
    mem === null
      ? { key: "mem", pct: null, text: "mem —", title: "machine memory: total unknown" }
      : {
          key: "mem",
          pct: mem,
          text: `mem ${Math.round(mem)}%`,
          title: `machine memory: ${formatBytes(stats.mem_used_bytes)} of ${formatBytes(stats.mem_total_bytes)} used (${Math.round(mem)}%)`,
        },
  ];
  if (showCount) {
    items.push({
      key: "count",
      pct: null,
      text: countText(stats.claude_count),
      title: "Claude Code processes running",
    });
  }
  return items;
}

/**
 * The whole rendering decision for the system line, per the spec §4.4 state
 * table. Rows 1 to 3 are exclusive; row 4 is any stats; rows 5 to 7 are dim
 * modifiers on row 4, the first true one supplying the reason.
 */
export function systemLine(input: {
  report: SystemReport | null;
  error: string | null;
  showCount: boolean;
  now: number;
}): SysLine {
  const { report, error, showCount, now } = input;
  const stats = report?.stats ?? null;
  const stopped = report?.stopped ?? false;

  if (stats === null) {
    if (error !== null) {
      return {
        items: [{ key: "unavailable", pct: null, text: "system usage unavailable", title: error }],
        dimmed: false,
      };
    }
    if (stopped) {
      return {
        items: [
          { key: "unavailable", pct: null, text: "system usage unavailable", title: SAMPLER_STOPPED },
        ],
        dimmed: false,
      };
    }
    return {
      items: [
        {
          key: "waiting",
          pct: null,
          text: "waiting for first sample",
          title: "the first figures arrive within a second of launch",
        },
      ],
      dimmed: false,
    };
  }

  const items = liveItems(stats, showCount);

  let reason: string | null = null;
  if (error !== null) reason = error;
  else if (stopped) reason = SAMPLER_STOPPED;
  else if (isStale(stats, now)) reason = `last sample ${clock(stats.sampled_at)}`;

  if (reason === null) return { items, dimmed: false };
  return {
    items: items.map((i) => ({ ...i, title: `${i.title} · ${reason}` })),
    dimmed: true,
  };
}
