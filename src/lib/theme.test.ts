import { describe, expect, it } from "vitest";
import {
  FONTS, FONT_KEYS, SIZES, SIZE_KEYS, THEME, THRESHOLDS,
  isFontKey, isSizeKey, metricColor, metricTone,
} from "./theme";

describe("metricTone", () => {
  it("is ok below the warn threshold", () => {
    expect(metricTone(0)).toBe("ok");
    expect(metricTone(69)).toBe("ok");
  });
  it("is warn from 70 up to but excluding 95", () => {
    expect(metricTone(THRESHOLDS.warn)).toBe("warn");
    expect(metricTone(94)).toBe("warn");
  });
  it("is crit at 95 and above, including over 100", () => {
    expect(metricTone(THRESHOLDS.crit)).toBe("crit");
    expect(metricTone(100)).toBe("crit");
    expect(metricTone(140)).toBe("crit");
  });
  it("maps tones to the theme colours", () => {
    expect(metricColor(10)).toBe(THEME.ok);
    expect(metricColor(80)).toBe(THEME.warn);
    expect(metricColor(99)).toBe(THEME.crit);
  });
});

describe("font and size keys", () => {
  it("lists every key of the maps in a stable order", () => {
    expect(FONT_KEYS).toEqual(["system", "plex", "jetbrains"]);
    expect(SIZE_KEYS).toEqual(["sm", "md", "lg", "xl"]);
    expect(Object.keys(FONTS).sort()).toEqual([...FONT_KEYS].sort());
    expect(Object.keys(SIZES).sort()).toEqual([...SIZE_KEYS].sort());
  });
  it("guards accept only known keys", () => {
    expect(isFontKey("plex")).toBe(true);
    expect(isFontKey("comic")).toBe(false);
    expect(isFontKey(3)).toBe(false);
    expect(isSizeKey("xl")).toBe(true);
    expect(isSizeKey("xxl")).toBe(false);
    expect(isSizeKey(null)).toBe(false);
  });
  it("default size has zoom 1 and every zoom is positive", () => {
    expect(SIZES.md.zoom).toBe(1);
    for (const key of SIZE_KEYS) expect(SIZES[key].zoom).toBeGreaterThan(0);
  });
});
