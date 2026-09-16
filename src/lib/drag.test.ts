import { describe, expect, it } from "vitest";
import { colDragTarget, colLineX, rowDragTarget, rowShift, settlePendingRows, toLocal } from "./drag";

describe("rowDragTarget", () => {
  it("rounds the displacement to whole rows and clamps to the list", () => {
    expect(rowDragTarget(1, 0, 66, 3)).toBe(1);
    expect(rowDragTarget(1, 40, 66, 3)).toBe(2);
    expect(rowDragTarget(1, 32, 66, 3)).toBe(1);
    expect(rowDragTarget(1, -500, 66, 3)).toBe(0);
    expect(rowDragTarget(1, 500, 66, 3)).toBe(2);
  });
  it("clamps to index 0 for an empty list", () => {
    expect(rowDragTarget(0, 500, 66, 0)).toBe(0);
  });
});

describe("toLocal", () => {
  it("converts a screen-space pixel quantity into local (unzoomed) px", () => {
    expect(toLocal(122, 1.22)).toBeCloseTo(100);
    expect(toLocal(50, 1)).toBe(50);
  });
  it("treats a zero or non-finite zoom as 1", () => {
    expect(toLocal(50, 0)).toBe(50);
    expect(toLocal(50, NaN)).toBe(50);
  });
});

describe("rowShift", () => {
  it("moves rows between the origin and the target out of the way", () => {
    expect(rowShift(1, 0, 2, 66)).toBe(-66);
    expect(rowShift(2, 0, 2, 66)).toBe(-66);
    expect(rowShift(3, 0, 2, 66)).toBe(0);
    expect(rowShift(1, 2, 0, 66)).toBe(66);
    expect(rowShift(0, 2, 1, 66)).toBe(0);
    expect(rowShift(2, 2, 0, 66)).toBe(0);
  });
});

const rects = [
  { left: 0, right: 100, width: 100 },
  { left: 110, right: 210, width: 100 },
  { left: 220, right: 320, width: 100 },
];

describe("colDragTarget", () => {
  it("picks the first column whose midpoint is right of the pointer", () => {
    expect(colDragTarget(rects, 10)).toBe(0);
    expect(colDragTarget(rects, 60)).toBe(1);
    expect(colDragTarget(rects, 200)).toBe(2);
    expect(colDragTarget(rects, 999)).toBe(2);
    expect(colDragTarget([], 5)).toBe(0);
  });
});

describe("colLineX", () => {
  it("draws before the target when moving left, after it when moving right", () => {
    expect(colLineX(rects, 0, 2, 20)).toBe(-25);
    expect(colLineX(rects, 2, 0, 20)).toBe(305);
    expect(colLineX(rects, 1, 1, 0)).toBe(105);
    expect(colLineX(rects, 7, 0, 0)).toBe(0);
  });
});

describe("settlePendingRows", () => {
  it("passes a stashed rows update through when no refetch will resync it", () => {
    // Covers both a non-committing drag AND a committed column reorder:
    // reordering columns only writes local prefs and never refetches
    // rows, so a column drag always passes `refetchWillResync: false`,
    // even when it committed.
    expect(settlePendingRows(["a", "b"], false)).toEqual(["a", "b"]);
  });
  it("discards the stash when a refetch will resync it (a committed row reorder)", () => {
    expect(settlePendingRows(["a", "b"], true)).toBeNull();
  });
  it("is a no-op with nothing stashed, either way", () => {
    expect(settlePendingRows(null, false)).toBeNull();
    expect(settlePendingRows(null, true)).toBeNull();
  });
});
