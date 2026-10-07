/// <reference types="node" />
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  BASE_WIDTH,
  ROW_BORDER,
  ROW_HEIGHT,
  ZOOM_MAX,
  ZOOM_MIN,
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

describe("the density constants", () => {
  it("ROW_HEIGHT is 40", () => {
    expect(ROW_HEIGHT).toBe(40);
  });
  it("ROW_BORDER is 1", () => {
    expect(ROW_BORDER).toBe(1);
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
