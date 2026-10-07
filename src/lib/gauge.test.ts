import { describe, expect, it } from "vitest";
import { RING_SIZES, ringDash, ringViewBox } from "./gauge";

describe("ringDash", () => {
  const c = 2 * Math.PI * RING_SIZES.sm.radius;
  it("hides the arc for null, shows it in proportion, and clamps at 100", () => {
    expect(ringDash(null, RING_SIZES.sm.radius)).toEqual({ circumference: c, offset: c });
    expect(ringDash(0, RING_SIZES.sm.radius).offset).toBeCloseTo(c);
    expect(ringDash(50, RING_SIZES.sm.radius).offset).toBeCloseTo(c / 2);
    expect(ringDash(100, RING_SIZES.sm.radius).offset).toBeCloseTo(0);
    expect(ringDash(140, RING_SIZES.sm.radius).offset).toBeCloseTo(0);
    expect(ringDash(-5, RING_SIZES.sm.radius).offset).toBeCloseTo(c);
  });
});

describe("RING_SIZES.sm", () => {
  it("sm is the 20px variant", () => {
    expect(RING_SIZES.sm).toEqual({ size: 20, stroke: 3, radius: 8.5 });
  });

  it("dashes the small radius at 0, 50 and 100 percent", () => {
    const r = RING_SIZES.sm.radius;
    const full = 2 * Math.PI * r;
    expect(ringDash(0, r).offset).toBeCloseTo(full, 5);
    expect(ringDash(50, r).offset).toBeCloseTo(full / 2, 5);
    expect(ringDash(100, r).offset).toBeCloseTo(0, 5);
    expect(ringDash(null, r).offset).toBeCloseTo(full, 5);
  });
});

describe("RING_SIZES.md", () => {
  it("md is the 44px card ring with a 5px stroke", () => {
    expect(RING_SIZES.md).toEqual({ size: 44, stroke: 5, radius: 19.5 });
  });
  it("md arc plus half its stroke fits inside its box", () => {
    const { size, stroke, radius } = RING_SIZES.md;
    expect(radius + stroke / 2).toBeLessThanOrEqual(size / 2);
  });
  it("dashes the md radius at 0, 50 and 100 percent", () => {
    const r = RING_SIZES.md.radius;
    const full = 2 * Math.PI * r;
    expect(ringDash(0, r).offset).toBeCloseTo(full, 5);
    expect(ringDash(50, r).offset).toBeCloseTo(full / 2, 5);
    expect(ringDash(100, r).offset).toBeCloseTo(0, 5);
    expect(ringDash(null, r).offset).toBeCloseTo(full, 5);
  });
});

describe("ringViewBox", () => {
  it("is the geometry box, so the stroke scales with the element", () => {
    expect(ringViewBox(RING_SIZES.md)).toBe("0 0 44 44");
    expect(ringViewBox(RING_SIZES.sm)).toBe("0 0 20 20");
  });
});
