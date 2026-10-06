import { describe, expect, it } from "vitest";
import { DEFAULT_ORDER, gridMinWidth, visibleColumns } from "./columns";
import {
  BREAKPOINTS,
  ROW_BORDER,
  ROW_HEIGHT,
  SHELL_PADDING,
  autoHiddenColumns,
  layoutFor,
  shellStyle,
  shellVars,
} from "./layout";

describe("layoutFor", () => {
  it("switches at the breakpoints, measured in local px", () => {
    expect(BREAKPOINTS).toEqual({ narrow: 757, cards: 573 });
    expect(layoutFor(757, 1)).toBe("full");
    expect(layoutFor(756, 1)).toBe("narrow");
    expect(layoutFor(573, 1)).toBe("narrow");
    expect(layoutFor(572, 1)).toBe("cards");
  });
  it("divides the viewport width by the text-size zoom", () => {
    expect(layoutFor(924, 1.22)).toBe("full");
    expect(layoutFor(923, 1.22)).toBe("narrow");
    expect(layoutFor(700, 1.22)).toBe("narrow");
    expect(layoutFor(699, 1.22)).toBe("cards");
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

describe("the density constants", () => {
  it("SHELL_PADDING is 2*6 + 2*1 + 2*8 + 17", () => {
    expect(SHELL_PADDING).toBe(47);
  });
  it("ROW_HEIGHT is 40", () => {
    expect(ROW_HEIGHT).toBe(40);
  });
  it("ROW_BORDER is 1", () => {
    expect(ROW_BORDER).toBe(1);
  });
  it("the table fits exactly at each computed breakpoint", () => {
    expect(gridMinWidth(DEFAULT_ORDER) + SHELL_PADDING).toBe(BREAKPOINTS.narrow);
    const narrow = visibleColumns(DEFAULT_ORDER, autoHiddenColumns("narrow"));
    expect(gridMinWidth(narrow) + SHELL_PADDING).toBe(BREAKPOINTS.cards);
  });
});

describe("shellVars and shellStyle", () => {
  it("shellVars carries every shared length as a px custom property", () => {
    expect(shellVars()).toEqual({
      "--gutter": "6px",
      "--row-h": "40px",
      "--row-border": "1px",
      "--row-pad-x": "8px",
      "--grid-gap": "8px",
      "--panel-border": "1px",
      "--ring-min": "36px",
      "--ring-max": "120px",
    });
  });
  it("shellStyle sets the zoom and spreads every shell variable", () => {
    const style = shellStyle(1.22) as Record<string, unknown>;
    expect(style["zoom"]).toBe(1.22);
    for (const [name, value] of Object.entries(shellVars())) {
      expect(style[name]).toBe(value);
    }
  });
});
