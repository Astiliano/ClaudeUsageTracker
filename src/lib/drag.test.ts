import { describe, expect, it } from "vitest";
import { colDragTarget, colLineX, rowDragTarget, rowShift } from "./drag";

describe("rowDragTarget", () => {
  it("rounds the displacement to whole rows and clamps to the list", () => {
    expect(rowDragTarget(1, 0, 66, 3)).toBe(1);
    expect(rowDragTarget(1, 40, 66, 3)).toBe(2);
    expect(rowDragTarget(1, 32, 66, 3)).toBe(1);
    expect(rowDragTarget(1, -500, 66, 3)).toBe(0);
    expect(rowDragTarget(1, 500, 66, 3)).toBe(2);
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
