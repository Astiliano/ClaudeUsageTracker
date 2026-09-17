import { describe, expect, it } from "vitest";
import { RING, RING_SIZES, ringDash } from "./gauge";

describe("ringDash", () => {
  const c = 2 * Math.PI * RING.radius;
  it("hides the arc for null, shows it in proportion, and clamps at 100", () => {
    expect(ringDash(null, RING.radius)).toEqual({ circumference: c, offset: c });
    expect(ringDash(0, RING.radius).offset).toBeCloseTo(c);
    expect(ringDash(50, RING.radius).offset).toBeCloseTo(c / 2);
    expect(ringDash(100, RING.radius).offset).toBeCloseTo(0);
    expect(ringDash(140, RING.radius).offset).toBeCloseTo(0);
    expect(ringDash(-5, RING.radius).offset).toBeCloseTo(c);
  });
  it("ring geometry fits the 44px box with a 5px stroke", () => {
    expect(RING).toEqual({ size: 44, stroke: 5, radius: 19.5 });
    expect(RING.radius + RING.stroke / 2).toBeLessThanOrEqual(RING.size / 2);
  });
});

describe("RING_SIZES.sm", () => {
  it("keeps the md geometry and adds a 20px variant", () => {
    expect(RING_SIZES.md).toEqual({ size: 44, stroke: 5, radius: 19.5 });
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
