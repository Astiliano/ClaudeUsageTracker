import { describe, expect, it } from "vitest";
import { seriesDots, seriesLine, seriesStats } from "./series";

describe("seriesLine", () => {
  it("draws straight through a null slot instead of breaking", () => {
    expect(seriesLine([0, null, 100], 100, 24)).toBe("0.00,24.00 100.00,0.00");
  });
  it("skips leading and trailing nulls but keeps the slot positions", () => {
    // n = 4, so slots 1 and 2 sit at x = 33.33 and 66.67.
    expect(seriesLine([null, 10, 20, null], 100, 24)).toBe("33.33,21.60 66.67,19.20");
  });
  it("places a single known value at its slot", () => {
    expect(seriesLine([50], 100, 24)).toBe("0.00,12.00");
    expect(seriesLine([null, 50], 100, 24)).toBe("100.00,12.00");
  });
  it("is empty for an empty series or a series with no known values", () => {
    expect(seriesLine([], 100, 24)).toBe("");
    expect(seriesLine([null, null], 100, 24)).toBe("");
  });
  it("clamps values into 0..100", () => {
    expect(seriesLine([150, -10], 100, 100)).toBe("0.00,0.00 100.00,100.00");
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
