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

function stats(over: Partial<SystemStats> = {}): SystemStats {
  return {
    sampled_at: 1_000_000,
    cpu_pct: 12.4,
    mem_used_bytes: Math.round(13.1 * GIB),
    mem_total_bytes: 32 * GIB,
    claude_count: 2,
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
  it("is used over total, clamped", () => {
    expect(memPct(stats())).toBeCloseTo(40.94, 1);
    expect(memPct(stats({ mem_total_bytes: 0 }))).toBeNull();
    expect(memPct(stats({ mem_total_bytes: GIB, mem_used_bytes: 4 * GIB }))).toBe(100);
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

  it("row 4: stats give cpu and mem in percent, with the absolute figures as titles", () => {
    const line = systemLine({ report: report(stats()), error: null, showCount: false, now: NOW });
    expect(line.items.map((i) => i.key)).toEqual(["cpu", "mem"]);
    expect(line.items[0]).toMatchObject({ pct: 12.4, text: "cpu 12%", title: "machine CPU: 12% busy" });
    expect(line.items[1].pct).toBeCloseTo(40.94, 1);
    expect(line.items[1].text).toBe("mem 41%");
    expect(line.items[1].title).toBe("machine memory: 13.1 GB of 32.0 GB used (41%)");
    expect(line.dimmed).toBe(false);
  });

  it("row 4: the count item appears only when asked, with singular and zero forms", () => {
    const two = systemLine({ report: report(stats()), error: null, showCount: true, now: NOW });
    expect(two.items.map((i) => i.key)).toEqual(["cpu", "mem", "count"]);
    expect(two.items[2]).toMatchObject({ pct: null, text: "2 procs", title: "Claude Code processes running" });
    expect(systemLine({ report: report(stats({ claude_count: 1 })), error: null, showCount: true, now: NOW }).items[2].text).toBe("1 proc");
    expect(systemLine({ report: report(stats({ claude_count: 0 })), error: null, showCount: true, now: NOW }).items[2].text).toBe("0 procs");
  });

  it("row 4: a zero count still shows the machine figures", () => {
    const line = systemLine({ report: report(stats({ claude_count: 0 })), error: null, showCount: false, now: NOW });
    expect(line.items.map((i) => i.key)).toEqual(["cpu", "mem"]);
  });

  it("row 4: an unknown total is a dash", () => {
    const line = systemLine({ report: report(stats({ mem_total_bytes: 0 })), error: null, showCount: false, now: NOW });
    expect(line.items[1]).toMatchObject({ pct: null, text: "mem —", title: "machine memory: total unknown" });
  });

  it("row 5: an error dims live stats and supplies the reason", () => {
    const line = systemLine({ report: report(stats()), error: "boom", showCount: false, now: NOW });
    expect(line.items.map((i) => i.key)).toEqual(["cpu", "mem"]);
    expect(line.dimmed).toBe(true);
    expect(line.items[0].title.endsWith("boom")).toBe(true);
  });

  it("row 6: stopped dims live stats at once, before they turn stale", () => {
    const line = systemLine({ report: report(stats(), true), error: null, showCount: false, now: NOW });
    expect(line.dimmed).toBe(true);
    expect(line.items[0].title.endsWith("sampler stopped, see log")).toBe(true);
  });

  it("row 7: a stale sample dims and names its time", () => {
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
