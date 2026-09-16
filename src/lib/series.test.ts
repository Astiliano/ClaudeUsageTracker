import { describe, expect, it } from "vitest";
import { axisLabels, dailyMax, dayLabel, polylinePoints, seriesDots, seriesStats } from "./series";

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

describe("polylinePoints", () => {
  it("spaces known points by index and skips nulls without breaking the line", () => {
    expect(polylinePoints([0, null, 100], 100, 24)).toBe("0.00,24.00 100.00,0.00");
  });
  it("is empty with fewer than two known values", () => {
    expect(polylinePoints([50], 100, 24)).toBe("");
    expect(polylinePoints([null, null], 100, 24)).toBe("");
  });
  it("clamps values into 0..100", () => {
    expect(polylinePoints([150, -10], 100, 100)).toBe("0.00,0.00 100.00,100.00");
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
