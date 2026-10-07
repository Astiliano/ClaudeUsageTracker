/// <reference types="node" />
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { DEFAULT_ORDER, gridMinWidth, visibleColumns } from "./columns";
import {
  BASE_WIDTH,
  BREAKPOINTS,
  ROW_BORDER,
  ROW_HEIGHT,
  SHELL_PADDING,
  ZOOM_MAX,
  ZOOM_MIN,
  autoHiddenColumns,
  layoutFor,
  shellStyle,
  shellVars,
  windowZoom,
} from "./layout";

describe("windowZoom", () => {
  it("fitted window: both terms agree", () => {
    expect(windowZoom(1470, 960, 640)).toBeCloseTo(1.5, 10);
  });
  it("taller than needed: width term wins", () => {
    expect(windowZoom(1470, 980, 640)).toBeCloseTo(1.5, 10);
  });
  it("maximized: height term wins", () => {
    expect(windowZoom(1920, 1032, 900)).toBeCloseTo(1032 / 900, 10);
  });
  it("null contentH: width term only", () => {
    expect(windowZoom(1470, 300, null)).toBeCloseTo(1.5, 10);
  });
  it("bad contentH falls back to the width term", () => {
    for (const c of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
      expect(windowZoom(1470, 300, c)).toBeCloseTo(1.5, 10);
    }
  });
  it("bad width or height gives 1", () => {
    expect(windowZoom(0, 0, 640)).toBe(1);
    expect(windowZoom(0, 640, 640)).toBe(1);
    expect(windowZoom(980, 0, 640)).toBe(1);
    expect(windowZoom(-5, 640, 640)).toBe(1);
    expect(windowZoom(Number.NaN, 640, 640)).toBe(1);
    expect(windowZoom(980, Number.NaN, 640)).toBe(1);
    expect(windowZoom(Number.POSITIVE_INFINITY, 640, 640)).toBe(1);
  });
  it("clamps", () => {
    expect(windowZoom(500, 2000, 640)).toBe(ZOOM_MIN);
    expect(windowZoom(4000, 4000, 640)).toBe(ZOOM_MAX);
    expect(ZOOM_MIN).toBe(0.75);
    expect(ZOOM_MAX).toBe(2.5);
  });
  it("the window config matches the base canvas", () => {
    const conf = JSON.parse(
      readFileSync(new URL("../../src-tauri/tauri.conf.json", import.meta.url), "utf8"),
    ) as { app: { windows: { width: number; minWidth: number; minHeight?: number }[] } };
    const win = conf.app.windows[0];
    expect(win?.width).toBe(BASE_WIDTH);
    expect(win?.minWidth).toBe(BASE_WIDTH * ZOOM_MIN);
    expect(win?.minHeight).toBeUndefined();
  });
});

describe("layoutFor", () => {
  it("switches at the breakpoints, measured in local px", () => {
    expect(BREAKPOINTS).toEqual({ narrow: 757, cards: 573 });
    expect(layoutFor(757, 1)).toBe("full");
    expect(layoutFor(756, 1)).toBe("narrow");
    expect(layoutFor(573, 1)).toBe("narrow");
    expect(layoutFor(572, 1)).toBe("cards");
  });
  it("divides the viewport width by the zoom", () => {
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
      "--base-w": "980px",
      "--chart-h": "220px",
    });
  });
  it("shellStyle sets the zoom and spreads every shell variable", () => {
    const style = shellStyle(1.22, 900) as Record<string, unknown>;
    expect(style["zoom"]).toBe(1.22);
    for (const [name, value] of Object.entries(shellVars())) {
      expect(style[name]).toBe(value);
    }
  });
  it("shellStyle emits --viewport-h in local px (window px / zoom)", () => {
    expect((shellStyle(2, 1280) as Record<string, unknown>)["--viewport-h"]).toBe("640px");
    expect((shellStyle(1, 640) as Record<string, unknown>)["--viewport-h"]).toBe("640px");
  });
  it("shellStyle emits 0px for a zero, NaN or negative viewport height", () => {
    expect((shellStyle(1.5, 0) as Record<string, unknown>)["--viewport-h"]).toBe("0px");
    expect((shellStyle(1.5, Number.NaN) as Record<string, unknown>)["--viewport-h"]).toBe("0px");
    expect((shellStyle(1.5, -10) as Record<string, unknown>)["--viewport-h"]).toBe("0px");
  });
});
