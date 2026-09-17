import { describe, expect, it } from "vitest";
import { DEFAULT_ORDER, gridMinWidth, visibleColumns } from "./columns";
import { BREAKPOINTS, SHELL_PADDING, autoHiddenColumns, layoutFor } from "./layout";

describe("layoutFor", () => {
  it("switches at the breakpoints, measured in local px", () => {
    expect(BREAKPOINTS).toEqual({ narrow: 820, cards: 640 });
    expect(layoutFor(820, 1)).toBe("full");
    expect(layoutFor(819, 1)).toBe("narrow");
    expect(layoutFor(640, 1)).toBe("narrow");
    expect(layoutFor(639, 1)).toBe("cards");
  });
  it("divides the viewport width by the text-size zoom", () => {
    expect(layoutFor(1001, 1.22)).toBe("full");
    expect(layoutFor(1000, 1.22)).toBe("narrow");
    expect(layoutFor(781, 1.22)).toBe("narrow");
    expect(layoutFor(780, 1.22)).toBe("cards");
    expect(layoutFor(700, 0)).toBe("narrow"); // a bad zoom is treated as 1
  });
});

describe("autoHiddenColumns", () => {
  it("hides updated and model only in the narrow layout", () => {
    expect(autoHiddenColumns("full")).toEqual([]);
    expect(autoHiddenColumns("narrow")).toEqual(["updated", "model"]);
    expect(autoHiddenColumns("cards")).toEqual([]);
  });
});

describe("the table fits its band", () => {
  it("full table fits above the narrow breakpoint and the narrow table above the cards breakpoint", () => {
    expect(SHELL_PADDING).toBe(95);
    expect(gridMinWidth(DEFAULT_ORDER) + SHELL_PADDING).toBeLessThanOrEqual(BREAKPOINTS.narrow);
    const narrow = visibleColumns(DEFAULT_ORDER, autoHiddenColumns("narrow"));
    expect(gridMinWidth(narrow) + SHELL_PADDING).toBeLessThanOrEqual(BREAKPOINTS.cards);
  });
});
