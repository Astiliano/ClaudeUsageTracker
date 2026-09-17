import { describe, expect, it } from "vitest";
import {
  clock,
  formatBytes,
  isStale,
  memPct,
  processCountSuffix,
  STALE_AFTER_MS,
  systemLine,
} from "./system";
import type { SystemReport, SystemStats } from "./types";

const GIB = 1024 * 1024 * 1024;

function stats(over: Partial<SystemStats> = {}, claudeOver: Partial<SystemStats["claude"]> = {}): SystemStats {
  return {
    sampled_at: 1_000_000,
    mem_total_bytes: 32 * GIB,
    claude: { count: 2, rss_bytes: Math.round(1.2 * GIB), cpu_pct: 3, ...claudeOver },
    ...over,
  };
}

function report(s: SystemStats | null, stopped = false): SystemReport {
  return { stats: s, stopped };
}

const NOW = 1_000_000;

describe("formatBytes", () => {
  it("uses whole MiB under a GiB and one decimal at or above", () => {
    expect(formatBytes(0)).toBe("0 MB");
    expect(formatBytes(512 * 1024 * 1024)).toBe("512 MB");
    expect(formatBytes(GIB)).toBe("1.0 GB");
    expect(formatBytes(1.25 * GIB)).toBe("1.3 GB");
    expect(formatBytes(20 * GIB)).toBe("20.0 GB");
  });
});

describe("memPct", () => {
  it("is the share of total memory, clamped", () => {
    expect(memPct(stats())).toBeCloseTo(3.75, 2);
    expect(memPct(stats({ mem_total_bytes: 0 }))).toBeNull();
    expect(memPct(stats({ mem_total_bytes: GIB }, { rss_bytes: 4 * GIB }))).toBe(100);
  });
});

describe("isStale", () => {
  it("turns true strictly after the threshold", () => {
    expect(isStale(stats(), NOW + STALE_AFTER_MS)).toBe(false);
    expect(isStale(stats(), NOW + STALE_AFTER_MS + 1)).toBe(true);
  });
});

describe("clock", () => {
  it("zero-pads hours, minutes and seconds", () => {
    expect(clock(new Date(2026, 8, 17, 14, 2, 11).getTime())).toBe("14:02:11");
    expect(clock(new Date(2026, 8, 17, 9, 0, 5).getTime())).toBe("09:00:05");
  });
});

describe("processCountSuffix", () => {
  it("is empty for nothing and singular for one", () => {
    expect(processCountSuffix(null)).toBe("");
    expect(processCountSuffix(0)).toBe("");
    expect(processCountSuffix(1)).toBe(" · 1 Claude process");
    expect(processCountSuffix(2)).toBe(" · 2 Claude processes");
  });
});

describe("systemLine state table", () => {
  it("row 1: no stats with an error is unavailable, undimmed, with the error", () => {
    const line = systemLine({ report: null, error: "boom", showCount: false, now: NOW });
    expect(line.items.map((i) => i.key)).toEqual(["unavailable"]);
    expect(line.items[0].text).toBe("system usage unavailable");
    expect(line.items[0].title).toBe("boom");
    expect(line.dimmed).toBe(false);
  });

  it("row 2: no stats but stopped is unavailable with the sampler reason", () => {
    const line = systemLine({ report: report(null, true), error: null, showCount: false, now: NOW });
    expect(line.items.map((i) => i.key)).toEqual(["unavailable"]);
    expect(line.items[0].title).toBe("sampler stopped, see log");
    expect(line.dimmed).toBe(false);
  });

  it("row 3: no stats and no error is waiting", () => {
    expect(
      systemLine({ report: null, error: null, showCount: false, now: NOW }).items.map((i) => i.key),
    ).toEqual(["waiting"]);
    expect(
      systemLine({ report: report(null), error: null, showCount: false, now: NOW }).items.map((i) => i.key),
    ).toEqual(["waiting"]);
  });

  it("row 4: a zero count is one plain item whatever showCount says", () => {
    for (const showCount of [true, false]) {
      const line = systemLine({
        report: report(stats({}, { count: 0, rss_bytes: 0, cpu_pct: 0 })),
        error: null,
        showCount,
        now: NOW,
      });
      expect(line.items.map((i) => i.key)).toEqual(["none"]);
      expect(line.items[0].text).toBe("no Claude processes");
      expect(line.dimmed).toBe(false);
    }
  });

  it("row 5: a live count is cpu, mem and optionally the count", () => {
    const without = systemLine({ report: report(stats()), error: null, showCount: false, now: NOW });
    expect(without.items.map((i) => i.key)).toEqual(["cpu", "mem"]);
    expect(without.items[0].text).toBe("cpu 3%");
    expect(without.items[0].title).toBe("Claude processes: 3% of the machine's CPU");
    expect(without.items[1].text).toBe("mem 1.2 GB");
    expect(without.items[1].title).toBe("Claude processes: 1.2 GB of 32.0 GB (4%)");

    const with_ = systemLine({ report: report(stats()), error: null, showCount: true, now: NOW });
    expect(with_.items.map((i) => i.key)).toEqual(["cpu", "mem", "count"]);
    expect(with_.items[2].text).toBe("2 procs");
    expect(with_.items[2].title).toBe("Claude Code processes running");
  });

  it("row 5: one process is singular and a null share is a dash", () => {
    const line = systemLine({
      report: report(stats({}, { count: 1, cpu_pct: null })),
      error: null,
      showCount: true,
      now: NOW,
    });
    expect(line.items[0].text).toBe("cpu —");
    expect(line.items[0].pct).toBeNull();
    expect(line.items[2].text).toBe("1 proc");
  });

  it("row 6: an error dims live stats and supplies the reason", () => {
    const line = systemLine({ report: report(stats()), error: "boom", showCount: false, now: NOW });
    expect(line.items.map((i) => i.key)).toEqual(["cpu", "mem"]);
    expect(line.dimmed).toBe(true);
    expect(line.items[0].title.endsWith("boom")).toBe(true);
  });

  it("row 7: stopped dims live stats at once, before they turn stale", () => {
    const line = systemLine({ report: report(stats(), true), error: null, showCount: false, now: NOW });
    expect(line.dimmed).toBe(true);
    expect(line.items[0].title.endsWith("sampler stopped, see log")).toBe(true);
  });

  it("row 8: a stale sample dims and names its time", () => {
    // 2026-09-17 14:02:11 local, so the clock is pinned without a locale.
    const at = new Date(2026, 8, 17, 14, 2, 11).getTime();
    const line = systemLine({
      report: report(stats({ sampled_at: at })),
      error: null,
      showCount: false,
      now: at + STALE_AFTER_MS + 1,
    });
    expect(line.dimmed).toBe(true);
    expect(line.items[0].title.endsWith("last sample 14:02:11")).toBe(true);
  });

  it("an error outranks stopped and staleness", () => {
    const line = systemLine({
      report: report(stats(), true),
      error: "boom",
      showCount: false,
      now: NOW + STALE_AFTER_MS + 1,
    });
    expect(line.items[0].title.endsWith("boom")).toBe(true);
  });
});
