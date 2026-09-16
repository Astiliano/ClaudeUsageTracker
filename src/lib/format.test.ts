import { describe, expect, it } from "vitest";
import { formatAgo, formatCountdown } from "./format";

const NOW = 1_700_000_000_000;
const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

describe("formatCountdown", () => {
  it("renders an em dash when there is no reset instant", () => {
    expect(formatCountdown(null, NOW)).toBe("—");
  });

  it("clamps a past reset to 'resets now'", () => {
    expect(formatCountdown(NOW - 1, NOW)).toBe("resets now");
    expect(formatCountdown(NOW, NOW)).toBe("resets now");
    expect(formatCountdown(NOW - DAY, NOW)).toBe("resets now");
  });

  it("renders sub-minute gaps as less than a minute", () => {
    expect(formatCountdown(NOW + 1 * SECOND, NOW)).toBe("resets in <1m");
    expect(formatCountdown(NOW + 59 * SECOND, NOW)).toBe("resets in <1m");
  });

  it("renders minutes under an hour", () => {
    expect(formatCountdown(NOW + 1 * MINUTE, NOW)).toBe("resets in 1m");
    expect(formatCountdown(NOW + 42 * MINUTE, NOW)).toBe("resets in 42m");
    expect(formatCountdown(NOW + 59 * MINUTE + 59 * SECOND, NOW)).toBe(
      "resets in 59m",
    );
  });

  it("renders hours and minutes under a day", () => {
    expect(formatCountdown(NOW + 3 * HOUR + 12 * MINUTE, NOW)).toBe(
      "resets in 3h 12m",
    );
    expect(formatCountdown(NOW + 1 * HOUR, NOW)).toBe("resets in 1h 0m");
    expect(formatCountdown(NOW + 23 * HOUR + 59 * MINUTE, NOW)).toBe(
      "resets in 23h 59m",
    );
  });

  it("renders days and hours beyond a day", () => {
    expect(formatCountdown(NOW + 2 * DAY + 4 * HOUR, NOW)).toBe(
      "resets in 2d 4h",
    );
    expect(formatCountdown(NOW + 6 * DAY + 23 * HOUR, NOW)).toBe(
      "resets in 6d 23h",
    );
  });
});

describe("formatAgo", () => {
  it("says never when nothing has been recorded", () => {
    expect(formatAgo(null, NOW)).toBe("never");
  });

  it("treats a future timestamp as just now", () => {
    expect(formatAgo(NOW + 5 * SECOND, NOW)).toBe("just now");
  });

  it("renders seconds", () => {
    expect(formatAgo(NOW, NOW)).toBe("0 s ago");
    expect(formatAgo(NOW - 42 * SECOND, NOW)).toBe("42 s ago");
    expect(formatAgo(NOW - 59 * SECOND, NOW)).toBe("59 s ago");
  });

  it("renders minutes", () => {
    expect(formatAgo(NOW - 1 * MINUTE, NOW)).toBe("1 min ago");
    expect(formatAgo(NOW - 59 * MINUTE, NOW)).toBe("59 min ago");
  });

  it("renders hours", () => {
    expect(formatAgo(NOW - 1 * HOUR, NOW)).toBe("1 h ago");
    expect(formatAgo(NOW - 23 * HOUR, NOW)).toBe("23 h ago");
  });

  it("renders days", () => {
    expect(formatAgo(NOW - 1 * DAY, NOW)).toBe("1 d ago");
    expect(formatAgo(NOW - 9 * DAY, NOW)).toBe("9 d ago");
  });
});
