import { describe, expect, it } from "vitest";
import { buildSparklinePaths } from "./sparkline";
import type { HistoryPoint } from "./types";

const HOUR = 3_600_000;
const BASE = 100 * HOUR;

function pts(spec: Array<[number, number]>): HistoryPoint[] {
  return spec.map(([hours, pct]) => ({ t: BASE + hours * HOUR, pct }));
}

describe("buildSparklinePaths", () => {
  it("returns nothing for an empty series", () => {
    expect(buildSparklinePaths([], 100, 20)).toEqual([]);
  });

  it("returns one sub-path for contiguous hours", () => {
    const paths = buildSparklinePaths(pts([[0, 10], [1, 20], [2, 30]]), 100, 20);
    expect(paths).toHaveLength(1);
    expect(paths[0].startsWith("M ")).toBe(true);
    expect(paths[0].split("L")).toHaveLength(3);
  });

  it("breaks the line at a gap instead of drawing through it", () => {
    // Hours 0 and 1, then a three-hour hole, then hours 5 and 6.
    const paths = buildSparklinePaths(
      pts([[0, 10], [1, 20], [5, 30], [6, 40]]),
      100,
      20,
    );
    expect(paths).toHaveLength(2);
  });

  it("never emits a zero for a missing hour", () => {
    const paths = buildSparklinePaths(pts([[0, 50], [4, 50]]), 100, 20);
    expect(paths.join(" ")).not.toContain("20 ");
    expect(paths).toHaveLength(2);
  });

  it("maps a higher percentage to a smaller y so the line rises", () => {
    const [path] = buildSparklinePaths(pts([[0, 0], [1, 100]]), 100, 20);
    const ys = [...path.matchAll(/-?\d+(?:\.\d+)?\s+(-?\d+(?:\.\d+)?)/g)].map(
      (m) => Number(m[1]),
    );
    expect(ys[0]).toBeGreaterThan(ys[1]);
  });

  it("keeps every point inside the box", () => {
    const [path] = buildSparklinePaths(
      pts([[0, 0], [1, 50], [2, 100]]),
      120,
      24,
    );
    const numbers = [...path.matchAll(/(-?\d+(?:\.\d+)?)/g)].map((m) =>
      Number(m[1]),
    );
    for (let i = 0; i < numbers.length; i += 2) {
      expect(numbers[i]).toBeGreaterThanOrEqual(0);
      expect(numbers[i]).toBeLessThanOrEqual(120);
      expect(numbers[i + 1]).toBeGreaterThanOrEqual(0);
      expect(numbers[i + 1]).toBeLessThanOrEqual(24);
    }
  });

  it("renders a single point as its own degenerate sub-path", () => {
    const paths = buildSparklinePaths(pts([[3, 42]]), 100, 20);
    expect(paths).toHaveLength(1);
    expect(paths[0]).toMatch(/^M [\d.]+ [\d.]+ L [\d.]+ [\d.]+$/);
  });

  it("puts a lone point at the right edge of the box", () => {
    const [path] = buildSparklinePaths(pts([[3, 42]]), 100, 20);
    expect(path.startsWith("M 100 ")).toBe(true);
  });

  it("sorts unordered input before drawing", () => {
    const unordered = pts([[2, 30], [0, 10], [1, 20]]);
    const ordered = pts([[0, 10], [1, 20], [2, 30]]);
    expect(buildSparklinePaths(unordered, 100, 20)).toEqual(
      buildSparklinePaths(ordered, 100, 20),
    );
  });
});
