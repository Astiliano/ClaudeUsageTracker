import { describe, expect, it } from "vitest";
import { buildSparklinePath } from "./sparkline";
import type { HistoryPoint } from "./types";

const HOUR = 3_600_000;
const BASE = 100 * HOUR;

function pts(spec: Array<[number, number]>): HistoryPoint[] {
  return spec.map(([hours, pct]) => ({ t: BASE + hours * HOUR, pct }));
}

describe("buildSparklinePath", () => {
  it("returns null for an empty series", () => {
    expect(buildSparklinePath([], 100, 20)).toBeNull();
  });

  it("returns one path for contiguous hours", () => {
    const path = buildSparklinePath(pts([[0, 10], [1, 20], [2, 30]]), 100, 20);
    expect(path).not.toBeNull();
    expect(path?.match(/L /g)).toHaveLength(2);
  });

  it("draws straight through a gap instead of breaking or dropping to zero", () => {
    // Hours 0 and 1, a three-hour hole, then hours 5 and 6: still one path.
    const path = buildSparklinePath(pts([[0, 10], [1, 20], [5, 30], [6, 40]]), 100, 20);
    expect(path).not.toBeNull();
    expect(path?.startsWith("M ")).toBe(true);
    expect(path?.match(/L /g)).toHaveLength(3);
    // y = 20 is pct 0; nothing in this series is 0.
    expect(path).not.toMatch(/ 20(?:\s|$)/);
  });

  it("maps a higher percentage to a smaller y so the line rises", () => {
    const path = buildSparklinePath(pts([[0, 0], [1, 100]]), 100, 20) ?? "";
    const ys = [...path.matchAll(/-?\d+(?:\.\d+)?\s+(-?\d+(?:\.\d+)?)/g)].map((m) => Number(m[1]));
    expect(ys[0]).toBeGreaterThan(ys[1]);
  });

  it("keeps every point inside the box", () => {
    const path = buildSparklinePath(pts([[0, 0], [1, 50], [2, 100]]), 120, 24) ?? "";
    const numbers = [...path.matchAll(/(-?\d+(?:\.\d+)?)/g)].map((m) => Number(m[1]));
    for (let i = 0; i < numbers.length; i += 2) {
      expect(numbers[i]).toBeGreaterThanOrEqual(0);
      expect(numbers[i]).toBeLessThanOrEqual(120);
      expect(numbers[i + 1]).toBeGreaterThanOrEqual(0);
      expect(numbers[i + 1]).toBeLessThanOrEqual(24);
    }
  });

  it("renders a single point as a zero-length line at the right edge", () => {
    const path = buildSparklinePath(pts([[3, 42]]), 100, 20);
    expect(path).toMatch(/^M [\d.]+ [\d.]+ L [\d.]+ [\d.]+$/);
    expect(path?.startsWith("M 100 ")).toBe(true);
  });

  it("sorts unordered input before drawing", () => {
    const unordered = pts([[2, 30], [0, 10], [1, 20]]);
    const ordered = pts([[0, 10], [1, 20], [2, 30]]);
    expect(buildSparklinePath(unordered, 100, 20)).toEqual(buildSparklinePath(ordered, 100, 20));
  });
});
