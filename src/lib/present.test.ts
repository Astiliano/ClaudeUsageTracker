import { describe, expect, it } from "vitest";
import type { Pill } from "./pill";
import { accountCountLabel, accountDotColor, chipFor, countPlacement, sessionNote, summarizeModels, weekNote } from "./present";
import { THEME } from "./theme";
import type { AccountRow, Dashboard } from "./types";

function row(over: Partial<AccountRow["account"]> = {}, weekPct: number | null = 10): AccountRow {
  return {
    account: {
      id: "a", label: "a", config_dir: "C:/a", enabled: true, disabled_reason: null,
      created_at: 0, sort_order: 0, ...over,
    },
    latest: weekPct === null ? null : {
      id: 1, account_id: "a", taken_at: 0, outcome: "ok", session: null,
      week_all: { pct: weekPct, resets_at: null }, week_models: [], error: null, duration_ms: 1,
    },
    backoff_until: null,
  };
}
const ok: Pill = { kind: "outcome", tone: "success", label: "ok" };

describe("accountDotColor", () => {
  it("is idle when disabled regardless of anything else", () => {
    expect(accountDotColor(row({ enabled: false }, 100), { kind: "disabled", tone: "neutral", label: "disabled" })).toBe(THEME.idle);
  });
  it("follows the pill tone for faults, then the weekly limit, then live", () => {
    expect(accountDotColor(row(), { kind: "outcome", tone: "error", label: "guard tripped" })).toBe(THEME.crit);
    expect(accountDotColor(row(), { kind: "backoff", tone: "warn", label: "backing off" })).toBe(THEME.warn);
    expect(accountDotColor(row({}, 96), ok)).toBe(THEME.crit);
    expect(accountDotColor(row({}, 50), ok)).toBe(THEME.live);
  });
  it("is idle while an enabled account has never been polled", () => {
    expect(accountDotColor(row({}, null), { kind: "pending", tone: "neutral", label: "not polled yet" })).toBe(THEME.idle);
  });
});

describe("summarizeModels", () => {
  it("is null with no models", () => {
    expect(summarizeModels([])).toBeNull();
  });
  it("shows the highest model and counts the rest", () => {
    expect(summarizeModels([{ label: "Fable", pct: 47, resets_at: null }])).toEqual({ pct: 47, label: "Fable", note: "Fable", title: "Fable 47%" });
    expect(summarizeModels([
      { label: "Opus", pct: 12, resets_at: null },
      { label: "Fable", pct: 47, resets_at: null },
    ])).toEqual({ pct: 47, label: "Fable", note: "Fable · +1", title: "Opus 12% · Fable 47%" });
  });
});

describe("notes and labels", () => {
  it("weekNote counts down to the reset, names a missing reset, and warns at the limit", () => {
    const now = 1_000_000;
    const DAY = 86_400_000;
    const HOUR = 3_600_000;
    expect(weekNote(null, now)).toEqual({ text: "no data", warn: false });
    expect(weekNote({ pct: 96, resets_at: null }, now)).toEqual({ text: "no reset", warn: true });
    expect(weekNote({ pct: 40, resets_at: null }, now)).toEqual({ text: "no reset", warn: false });
    expect(weekNote({ pct: 40, resets_at: now + 2 * DAY + 3 * HOUR }, now)).toEqual({ text: "2d 3h left", warn: false });
    expect(weekNote({ pct: 95, resets_at: now + HOUR }, now)).toEqual({ text: "1h 0m left", warn: true });
  });
  it("sessionNote covers missing, idle and counting-down sessions", () => {
    expect(sessionNote(null, 0)).toBe("no data");
    expect(sessionNote({ pct: 0, resets_at: null }, 0)).toBe("idle");
    expect(sessionNote({ pct: 40, resets_at: 3_600_000 * 2 + 60_000 * 3 }, 0)).toBe("2h 3m left");
  });
  it("accountCountLabel pluralises", () => {
    expect(accountCountLabel(1)).toBe("1 account");
    expect(accountCountLabel(0)).toBe("0 accounts");
    expect(accountCountLabel(3)).toBe("3 accounts");
  });
});

describe("chipFor", () => {
  const base: Dashboard = {
    accounts: [row()], gate: "active", busy: false, halted: null, stalled_at: null,
    binary: { path: "C:/claude.exe", source: "local_bin" }, interval_secs: 60,
  };
  it("maps each banner kind to a dot and short text", () => {
    expect(chipFor(base)).toEqual({ dot: "live", text: "polling every 60 s" });
    expect(chipFor({ ...base, gate: "idle" })).toEqual({ dot: "idle", text: "idle · waits for Claude Code" });
    expect(chipFor({ ...base, halted: "guard" })).toEqual({ dot: "crit", text: "polling halted" });
    expect(chipFor({ ...base, stalled_at: 1 })).toEqual({ dot: "warn", text: "stalled, recovered" });
    expect(chipFor({ ...base, binary: { path: null, source: null } })).toEqual({ dot: "warn", text: "no claude binary" });
    expect(chipFor({ ...base, accounts: [row({ enabled: false })] })).toEqual({ dot: "warn", text: "no enabled accounts" });
  });

  it("appends the process count to the active and idle chips only", () => {
    expect(chipFor(base, 2).text).toBe("polling every 60 s · 2 Claude processes");
    expect(chipFor({ ...base, gate: "idle" }, 1).text).toBe("idle · waits for Claude Code · 1 Claude process");
    expect(chipFor({ ...base, halted: "guard" }, 2).text).toBe("polling halted");
    expect(chipFor({ ...base, stalled_at: 1 }, 2).text).toBe("stalled, recovered");
    expect(chipFor({ ...base, binary: { path: null, source: null } }, 2).text).toBe("no claude binary");
    expect(chipFor({ ...base, accounts: [row({ enabled: false })] }, 2).text).toBe("no enabled accounts");
  });

  it("leaves every chip unchanged when the count is unknown", () => {
    expect(chipFor(base, null).text).toBe("polling every 60 s");
    expect(chipFor({ ...base, gate: "idle" }, null).text).toBe("idle · waits for Claude Code");
  });
});

describe("countPlacement", () => {
  it("uses the chip only for active and idle at full width", () => {
    expect(countPlacement("active", false)).toBe("chip");
    expect(countPlacement("idle", false)).toBe("chip");
  });

  it("falls back to the line in cards and for every other chip kind", () => {
    expect(countPlacement("active", true)).toBe("line");
    expect(countPlacement("idle", true)).toBe("line");
    for (const kind of ["halted", "stalled", "no_binary", "no_accounts"] as const) {
      expect(countPlacement(kind, false)).toBe("line");
      expect(countPlacement(kind, true)).toBe("line");
    }
  });
});
