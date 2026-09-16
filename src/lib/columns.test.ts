import { describe, expect, it } from "vitest";
import { COLUMNS, DEFAULT_ORDER, gridTemplate, normalizeColumnOrder } from "./columns";

describe("gridTemplate", () => {
  it("leads with the grip column, ends with the action column, widths in order", () => {
    expect(gridTemplate(["account", "spark"])).toBe(
      `26px ${COLUMNS.account.width} ${COLUMNS.spark.width} 62px`,
    );
  });
  it("default order is the six design columns left to right", () => {
    expect(DEFAULT_ORDER).toEqual(["account", "session", "week", "model", "spark", "updated"]);
  });
});

describe("normalizeColumnOrder", () => {
  it("returns a full valid permutation unchanged", () => {
    const reversed = [...DEFAULT_ORDER].reverse();
    expect(normalizeColumnOrder(reversed)).toEqual(reversed);
  });
  it("keeps known keys in their stored order, drops unknowns and duplicates, appends missing keys in default order", () => {
    expect(normalizeColumnOrder(["spark", "account", "bogus", "account"])).toEqual([
      "spark", "account", "session", "week", "model", "updated",
    ]);
  });
  it("returns null for a non-array or an array of non-strings", () => {
    expect(normalizeColumnOrder(null)).toBeNull();
    expect(normalizeColumnOrder("x")).toBeNull();
    expect(normalizeColumnOrder([1, 2, 3])).toBeNull();
  });
});
