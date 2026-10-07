import { describe, expect, it } from "vitest";
import { THEME, THRESHOLDS, metricColor, metricTone } from "./theme";

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
