import { describe, expect, it } from "vitest";
import limits from "./historyLimits.json";
import {
  MAX_BUCKETS, MAX_RANGE_MS, MIN_BUCKET_MS, PRESETS, PRESET_KEYS, SPARK_PRESET, SPARK_UNIT, UNITS, UNIT_KEYS,
  WEEK_ALL, alignedSince, axisLabelsFor, bucketCount, bucketSeries, effectiveUnit, metricFromKey, metricKey,
  metricLabel, nearestKnownSlot, slotLabel, unitAllowed,
} from "./history";

const MIN = 60_000;
const HOUR = 3_600_000;
const DAY = 86_400_000;
// 2026-09-16 15:07:30.250 local
const NOW = new Date(2026, 8, 16, 15, 7, 30, 250).getTime();

describe("limits", () => {
  it("come from the shared JSON the Rust side is tested against", () => {
    expect(MIN_BUCKET_MS).toBe(limits.minBucketMs);
    expect(MAX_BUCKETS).toBe(limits.maxBuckets);
    expect(MAX_RANGE_MS).toBe(limits.maxRangeMs);
    expect(MIN_BUCKET_MS).toBe(60_000);
    expect(MAX_BUCKETS).toBe(1000);
    expect(MAX_RANGE_MS).toBe(30 * DAY);
  });
});

describe("presets and units", () => {
  it("match spec §3.2 exactly", () => {
    expect(PRESET_KEYS).toEqual(["1h", "6h", "12h", "24h", "7d", "30d"]);
    expect(UNIT_KEYS).toEqual(["1m", "5m", "15m", "1h", "1d"]);
    expect(Object.fromEntries(PRESET_KEYS.map((p) => [p, PRESETS[p].autoUnit]))).toEqual({
      "1h": "1m", "6h": "5m", "12h": "15m", "24h": "15m", "7d": "1h", "30d": "1d",
    });
    expect(Object.fromEntries(UNIT_KEYS.map((u) => [u, UNITS[u].ms]))).toEqual({
      "1m": MIN, "5m": 5 * MIN, "15m": 15 * MIN, "1h": HOUR, "1d": DAY,
    });
    const allowed = Object.fromEntries(PRESET_KEYS.map((p) => [p, UNIT_KEYS.filter((u) => unitAllowed(p, u))]));
    expect(allowed).toEqual({
      "1h": ["1m", "5m", "15m"],
      "6h": ["1m", "5m", "15m", "1h"],
      "12h": ["1m", "5m", "15m", "1h"],
      "24h": ["5m", "15m", "1h"],
      "7d": ["15m", "1h", "1d"],
      "30d": ["1h", "1d"],
    });
  });
  it("every auto unit is allowed and every allowed pair respects both server limits", () => {
    for (const p of PRESET_KEYS) {
      expect(unitAllowed(p, PRESETS[p].autoUnit)).toBe(true);
      for (const u of UNIT_KEYS) {
        if (!unitAllowed(p, u)) continue;
        expect(UNITS[u].ms).toBeGreaterThanOrEqual(MIN_BUCKET_MS);
        expect(Math.ceil(PRESETS[p].rangeMs / UNITS[u].ms)).toBeLessThanOrEqual(MAX_BUCKETS);
        expect(PRESETS[p].rangeMs).toBeLessThanOrEqual(MAX_RANGE_MS);
      }
    }
  });
  it("effectiveUnit honours an allowed override and falls back to auto otherwise", () => {
    expect(effectiveUnit("7d", null)).toBe("1h");
    expect(effectiveUnit("7d", "1d")).toBe("1d");
    expect(effectiveUnit("7d", "1m")).toBe("1h");
  });
});

describe("metric helpers", () => {
  it("labels, keys and parses metrics", () => {
    expect(metricLabel(WEEK_ALL)).toBe("weekly limit");
    expect(metricLabel({ kind: "session" })).toBe("session");
    expect(metricLabel({ kind: "model", label: "Fable" })).toBe("Fable");
    expect(metricKey({ kind: "model", label: "Fable" })).toBe("model:Fable");
    expect(metricKey(WEEK_ALL)).toBe("week_all");
    expect(metricFromKey("session")).toEqual({ kind: "session" });
    expect(metricFromKey("model:Fable")).toEqual({ kind: "model", label: "Fable" });
    expect(metricFromKey("model:")).toEqual(WEEK_ALL);
    expect(metricFromKey("bogus")).toEqual(WEEK_ALL);
  });
});

describe("alignedSince", () => {
  it("rounds up to the next unit boundary in local time and never exceeds the range", () => {
    const since1m = alignedSince(NOW, "1h", "1m");
    expect(new Date(since1m).getSeconds()).toBe(0);
    expect(new Date(since1m).getMilliseconds()).toBe(0);
    expect(NOW - since1m).toBeLessThanOrEqual(HOUR);
    expect(NOW - since1m).toBeGreaterThan(HOUR - MIN);

    const since15 = alignedSince(NOW, "12h", "15m");
    expect(new Date(since15).getMinutes() % 15).toBe(0);
    expect(NOW - since15).toBeLessThanOrEqual(12 * HOUR);

    const sinceH = alignedSince(NOW, "7d", "1h");
    expect(new Date(sinceH).getMinutes()).toBe(0);
    expect(NOW - sinceH).toBeLessThanOrEqual(7 * DAY);

    const sinceD = alignedSince(NOW, "30d", "1d");
    const d = new Date(sinceD);
    expect([d.getHours(), d.getMinutes()]).toEqual([0, 0]);
    expect(NOW - sinceD).toBeLessThanOrEqual(30 * DAY);
  });
  it("leaves a value already on the boundary where it is", () => {
    const onHour = new Date(2026, 8, 16, 15, 0, 0, 0).getTime();
    expect(alignedSince(onHour + HOUR, "1h", "1m")).toBe(onHour);
    const midnight = new Date(2026, 8, 16, 0, 0, 0, 0).getTime();
    expect(alignedSince(midnight + 7 * DAY, "7d", "1d")).toBe(midnight);
  });
  it("lands on the local day start for day units across a whole year (covers any DST change)", () => {
    for (let i = 0; i < 40; i++) {
      const now = new Date(2026, 0, 3 + i * 9, 13, 21, 0, 0).getTime();
      const s = alignedSince(now, "30d", "1d");
      // Compare against the day start recomputed from the result itself, so a
      // zone whose spring-forward happens at 00:00 (no midnight that day) passes.
      const dayStart = new Date(s);
      dayStart.setHours(0, 0, 0, 0);
      expect(s).toBe(dayStart.getTime());
      expect(now - s).toBeLessThanOrEqual(30 * DAY);
    }
  });
  it("bucketCount never exceeds MAX_BUCKETS and is at least 2 for any allowed pair", () => {
    for (const p of PRESET_KEYS) for (const u of UNIT_KEYS) {
      if (!unitAllowed(p, u)) continue;
      const since = alignedSince(NOW, p, u);
      expect(bucketCount(since, NOW, u)).toBeLessThanOrEqual(MAX_BUCKETS);
      expect(bucketCount(since, NOW, u)).toBeGreaterThanOrEqual(2);
    }
  });
});

describe("bucketSeries", () => {
  const since = 1_000_000;
  it("puts each point in its slot, keeps the max, leaves gaps null, ignores out-of-range", () => {
    const vals = bucketSeries(
      [
        { t: since, pct: 10 },
        { t: since + 2 * MIN, pct: 30 },
        { t: since + 2 * MIN + 1, pct: 25 },
        { t: since - 1, pct: 99 },
        { t: since + 4 * MIN, pct: 50 },
      ],
      since, "1m", 4,
    );
    expect(vals).toEqual([10, null, 30, null]);
  });
  it("is all null for no points", () => {
    expect(bucketSeries([], since, "1h", 3)).toEqual([null, null, null]);
  });
});

describe("labels", () => {
  const midnight = new Date(2026, 8, 16, 0, 0, 0, 0).getTime();
  const hour = new Date(2026, 8, 16, 14, 0, 0, 0).getTime();
  it("uses times for sub-day ranges and dates otherwise, first and last always present", () => {
    expect(axisLabelsFor(hour, "1m", 60, 4, "en-US")).toEqual(["14:00", "14:20", "14:39", "14:59"]);
    expect(axisLabelsFor(midnight - 6 * DAY, "1d", 7, 4, "en-US")).toEqual(["Sep 10", "Sep 12", "Sep 14", "Sep 16"]);
    expect(axisLabelsFor(midnight - 6 * DAY, "1h", 168, 2, "en-US")).toEqual(["Sep 10", "Sep 16"]);
    expect(axisLabelsFor(hour, "15m", 96, 1, "en-US")).toHaveLength(2);
    // Fewer slots than requested labels: one label per slot, no repeats.
    expect(axisLabelsFor(hour, "15m", 3, 4, "en-US")).toEqual(["14:00", "14:15", "14:30"]);
  });
  it("slot labels carry date and time for sub-day units and the date only for days", () => {
    expect(slotLabel(hour, "15m", 5, "en-US")).toBe("Sep 16 15:15");
    expect(slotLabel(midnight - 2 * DAY, "1d", 2, "en-US")).toBe("Sep 16");
  });
});

describe("nearestKnownSlot", () => {
  it("maps the fraction via round(f * (n-1)) then walks outward, lower index first on ties", () => {
    expect(nearestKnownSlot([1, 2, 3, 4, 5], 0.5)).toBe(2);
    expect(nearestKnownSlot([1, 2, null, 4, 5], 0.5)).toBe(1);
    expect(nearestKnownSlot([1, null, null, null, 5], 0.5)).toBe(0);
    expect(nearestKnownSlot([null, null, 3], 0)).toBe(2);
    expect(nearestKnownSlot([7], 0.9)).toBe(0);
    expect(nearestKnownSlot([null, null], 0.3)).toBeNull();
    expect(nearestKnownSlot([], 0.3)).toBeNull();
    expect(nearestKnownSlot([1, 2, 3], 1.7)).toBe(2);
    expect(nearestKnownSlot([1, 2, 3], -3)).toBe(0);
  });
});

describe("row sparkline window", () => {
  it("is 24 hours at 15-minute buckets, an allowed pair of at most 97 slots", () => {
    expect(SPARK_PRESET).toBe("24h");
    expect(SPARK_UNIT).toBe("15m");
    expect(unitAllowed(SPARK_PRESET, SPARK_UNIT)).toBe(true);
    for (const now of [NOW, NOW + 7 * MIN + 1, NOW + 14 * MIN + 59_999, NOW + 3 * HOUR]) {
      const since = alignedSince(now, SPARK_PRESET, SPARK_UNIT);
      expect(now - since).toBeLessThanOrEqual(PRESETS["24h"].rangeMs);
      expect(bucketCount(since, now, SPARK_UNIT)).toBeLessThanOrEqual(97);
    }
  });
});
