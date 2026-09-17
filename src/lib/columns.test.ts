import { describe, expect, it } from "vitest";
import { COLUMNS, DEFAULT_ORDER, GRID_GAP, gridMinWidth, gridTemplate, moveVisible, normalizeColumnOrder, normalizeHiddenColumns, visibleColumns } from "./columns";

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

describe("visibleColumns", () => {
  it("filters hidden keys and never drops account", () => {
    expect(visibleColumns(DEFAULT_ORDER, ["session", "updated"])).toEqual(["account", "week", "model", "spark"]);
    expect(visibleColumns(DEFAULT_ORDER, ["account"])).toEqual([...DEFAULT_ORDER]);
    expect(visibleColumns(DEFAULT_ORDER, [])).toEqual([...DEFAULT_ORDER]);
  });
});

describe("moveVisible", () => {
  it("equals a plain move when nothing is hidden", () => {
    expect(moveVisible(DEFAULT_ORDER, [], 4, 1)).toEqual(["account", "spark", "session", "week", "model", "updated"]);
  });
  it("moves only the dragged key and leaves hidden keys where they sit", () => {
    // full: account session week model spark updated; hidden: model, updated
    // visible: account session week spark; drag spark (3) to 1
    expect(moveVisible(DEFAULT_ORDER, ["model", "updated"], 3, 1)).toEqual(["account", "spark", "session", "week", "model", "updated"]);
  });
  it("is well defined when a hidden key precedes visible index 0", () => {
    const order = ["session", "account", "week", "spark", "model", "updated"] as const;
    // hidden: session; visible: account week spark model updated; drag week (1) to 0
    expect(moveVisible(order, ["session"], 1, 0)).toEqual(["session", "week", "account", "spark", "model", "updated"]);
  });
  it("a drag made under the narrow auto-hidden set, then widened, shows only the dragged key moved", () => {
    const narrowHidden = ["updated", "model"] as const;
    const after = moveVisible(DEFAULT_ORDER, narrowHidden, 3, 0); // spark to the front
    expect(after).toEqual(["spark", "account", "session", "week", "model", "updated"]);
    expect(visibleColumns(after, [])).toEqual(after);
  });
  it("returns the order unchanged for out-of-range indices", () => {
    expect(moveVisible(DEFAULT_ORDER, ["model"], 5, 0)).toEqual([...DEFAULT_ORDER]);
    expect(moveVisible(DEFAULT_ORDER, [], -1, 0)).toEqual([...DEFAULT_ORDER]);
  });
});

describe("normalizeHiddenColumns", () => {
  it("keeps known keys except account, drops unknowns and duplicates, preserves order", () => {
    expect(normalizeHiddenColumns(["updated", "account", "bogus", "session", "updated"])).toEqual(["updated", "session"]);
    expect(normalizeHiddenColumns([])).toEqual([]);
  });
  it("returns null for a non-array or non-string elements", () => {
    expect(normalizeHiddenColumns(null)).toBeNull();
    expect(normalizeHiddenColumns("session")).toBeNull();
    expect(normalizeHiddenColumns([1])).toBeNull();
  });
});

describe("gridMinWidth", () => {
  it("sums track minimums plus the gaps between tracks", () => {
    expect(GRID_GAP).toBe(10);
    // 26 + 150 + 3*86 + 76 + 82 + 62 = 654 tracks, 7 gaps
    expect(gridMinWidth(DEFAULT_ORDER)).toBe(654 + 7 * GRID_GAP);
    // 26 + 150 + 86 + 86 + 76 + 62 = 486 tracks, 5 gaps
    expect(gridMinWidth(["account", "session", "week", "spark"])).toBe(486 + 5 * GRID_GAP);
    expect(COLUMNS.account.width).toBe("minmax(150px,1fr)");
  });
});
