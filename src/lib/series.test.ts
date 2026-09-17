import { describe, expect, it } from "vitest";
import { polylineRuns, seriesDots, seriesStats } from "./series";

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
