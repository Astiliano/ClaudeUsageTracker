import { describe, expect, it } from "vitest";
import { HISTORY_DAYS, axisLabels, dailyMax, dayLabel, polylineRuns, seriesDots, seriesStats } from "./series";

const HOUR = 3_600_000;
// A fixed local instant: 2026-09-16 15:00 local.
const NOW = new Date(2026, 8, 16, 15, 0, 0).getTime();
const dayStart = (offset: number): number => new Date(2026, 8, 16 + offset, 0, 0, 0).getTime();

describe("dailyMax", () => {
  it("puts today in the last bucket and takes the max per local day", () => {
    const out = dailyMax(
      [
        { t: dayStart(0) + 9 * HOUR, pct: 40 },
        { t: dayStart(0) + 13 * HOUR, pct: 46 },
        { t: dayStart(-1) + 22 * HOUR, pct: 12 },
      ],
      3,
      NOW,
    );
    expect(out).toEqual([null, 12, 46]);
  });
  it("ignores points outside the window and handles an empty series", () => {
    expect(dailyMax([{ t: dayStart(-5), pct: 99 }], 3, NOW)).toEqual([null, null, null]);
    expect(dailyMax([], 2, NOW)).toEqual([null, null]);
  });
  it("counts a point at 23:59 as its own day, not the next one", () => {
    const out = dailyMax([{ t: dayStart(0) - 60_000, pct: 7 }], 2, NOW);
    expect(out).toEqual([7, null]);
  });
});

describe("polylineRuns", () => {
  it("emits one run per contiguous span of known values, breaking across nulls", () => {
    expect(polylineRuns([0, null, 100], 100, 24)).toEqual(["0.00,24.00", "100.00,0.00"]);
  });
  it("emits a single-point run for an isolated known value amid gaps", () => {
    expect(polylineRuns([10, 20, null, 30], 100, 24)).toEqual([
      "0.00,21.60 33.33,19.20",
      "100.00,16.80",
    ]);
  });
  it("is empty for an empty series or a series with no known values", () => {
    expect(polylineRuns([], 100, 24)).toEqual([]);
    expect(polylineRuns([null, null], 100, 24)).toEqual([]);
  });
  it("clamps values into 0..100", () => {
    expect(polylineRuns([150, -10], 100, 100)).toEqual(["0.00,0.00 100.00,100.00"]);
  });
});

describe("HISTORY_DAYS", () => {
  it("is the single source of truth for the drawer window", () => {
    expect(HISTORY_DAYS).toBe(30);
  });
});

describe("seriesDots / seriesStats", () => {
  it("emits one dot per known value with its left percentage", () => {
    expect(seriesDots([10, null, 30])).toEqual([
      { index: 0, value: 10, leftPct: 0 },
      { index: 2, value: 30, leftPct: 100 },
    ]);
    expect(seriesDots([5])).toEqual([{ index: 0, value: 5, leftPct: 0 }]);
  });
  it("computes peak, rounded average and missing count", () => {
    expect(seriesStats([10, null, 31])).toEqual({ peak: 31, avg: 21, missing: 1 });
    expect(seriesStats([null])).toEqual({ peak: 0, avg: 0, missing: 1 });
  });
  it("is all zeros for an empty series", () => {
    expect(seriesStats([])).toEqual({ peak: 0, avg: 0, missing: 0 });
  });
});

describe("dayLabel / axisLabels", () => {
  it("labels today and earlier days", () => {
    expect(dayLabel(29, 30, NOW, "en-US")).toBe("Sep 16");
    expect(dayLabel(0, 30, NOW, "en-US")).toBe("Aug 18");
  });
  it("spreads count labels from oldest to today", () => {
    expect(axisLabels(30, 4, NOW, "en-US")).toEqual(["Aug 18", "Aug 28", "Sep 6", "Sep 16"]);
  });
});
