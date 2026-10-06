import { describe, expect, it } from "vitest";
import { CHART_MAX_PX, CHART_MIN_PX, chartHeightPx } from "./chart";

describe("chartHeightPx", () => {
  it("fills a 1600x900 window at zoom 1 (row 41 + chrome 86 + chart 773)", () => {
    expect(chartHeightPx({ viewportPx: 900, chromePx: 86, zoom: 1 })).toBe(773);
  });
  it("leaves 113 px at the 573x240 minimum window", () => {
    expect(chartHeightPx({ viewportPx: 240, chromePx: 86, zoom: 1 })).toBe(113);
  });
  it("never goes below CHART_MIN_PX", () => {
    expect(chartHeightPx({ viewportPx: 240, chromePx: 200, zoom: 1 })).toBe(CHART_MIN_PX);
  });
  it("never goes above CHART_MAX_PX", () => {
    expect(chartHeightPx({ viewportPx: 3000, chromePx: 86, zoom: 1 })).toBe(CHART_MAX_PX);
  });
  it("converts post-zoom viewport and chrome to local px at zoom 1.22", () => {
    expect(chartHeightPx({ viewportPx: 900, chromePx: 105, zoom: 1.22 })).toBe(
      Math.floor(900 / 1.22 - 41 - 105 / 1.22),
    );
  });
  it("fills a 1600x900 window at zoom 1.22 (hand-computed 610, not the formula)", () => {
    expect(chartHeightPx({ viewportPx: 900, chromePx: 104.92, zoom: 1.22 })).toBe(610);
  });
  it("fills a 1600x900 window at zoom 0.92 (hand-computed 851)", () => {
    expect(chartHeightPx({ viewportPx: 900, chromePx: 79.12, zoom: 0.92 })).toBe(851);
  });
  it("floors a 700x240 window at zoom 1.22 to CHART_MIN_PX (fit 69.7)", () => {
    expect(chartHeightPx({ viewportPx: 240, chromePx: 104.92, zoom: 1.22 })).toBe(CHART_MIN_PX);
  });
  it("exposes the spec bounds", () => {
    expect(CHART_MIN_PX).toBe(72);
    expect(CHART_MAX_PX).toBe(1200);
  });
});
