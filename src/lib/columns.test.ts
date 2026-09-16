import { describe, expect, it } from "vitest";
import { COLUMNS, DEFAULT_ORDER, gridTemplate, isColumnOrder } from "./columns";

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

describe("isColumnOrder", () => {
  it("accepts a permutation of every column exactly once", () => {
    expect(isColumnOrder([...DEFAULT_ORDER].reverse())).toBe(true);
  });
  it("rejects missing, duplicated or unknown keys and non-arrays", () => {
    expect(isColumnOrder(DEFAULT_ORDER.slice(1))).toBe(false);
    expect(isColumnOrder([...DEFAULT_ORDER, "account"])).toBe(false);
    expect(isColumnOrder(["account", "session", "week", "model", "spark", "cost"])).toBe(false);
    expect(isColumnOrder("account")).toBe(false);
    expect(isColumnOrder(null)).toBe(false);
  });
});
