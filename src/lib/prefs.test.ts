import { afterEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_ORDER } from "./columns";
import { DEFAULT_PREFS, PREFS_KEY, loadPrefs, parsePrefs, savePrefs } from "./prefs";

class MemoryStore {
  data = new Map<string, string>();
  getItem(k: string): string | null { return this.data.get(k) ?? null; }
  setItem(k: string, v: string): void { this.data.set(k, v); }
}

afterEach(() => vi.restoreAllMocks());

describe("parsePrefs", () => {
  it("returns defaults for null, garbage and non-object JSON", () => {
    expect(parsePrefs(null)).toEqual(DEFAULT_PREFS);
    expect(parsePrefs("{not json")).toEqual(DEFAULT_PREFS);
    expect(parsePrefs("42")).toEqual(DEFAULT_PREFS);
  });
  it("keeps valid fields and defaults invalid ones independently", () => {
    const parsed = parsePrefs(JSON.stringify({ font: "plex", size: "huge", columnOrder: ["account"] }));
    expect(parsed).toEqual({ font: "plex", size: "md", columnOrder: [...DEFAULT_ORDER], hiddenColumns: [], alwaysOnTop: false });
  });
  it("accepts a full valid record", () => {
    const order = [...DEFAULT_ORDER].reverse();
    const raw = JSON.stringify({ font: "jetbrains", size: "xl", columnOrder: order, hiddenColumns: ["session"], alwaysOnTop: true });
    expect(parsePrefs(raw)).toEqual({ font: "jetbrains", size: "xl", columnOrder: order, hiddenColumns: ["session"], alwaysOnTop: true });
  });
  it("never aliases the default column order array", () => {
    const a = parsePrefs(null);
    a.columnOrder.reverse();
    expect(parsePrefs(null).columnOrder).toEqual([...DEFAULT_ORDER]);
  });
  it("warns when a stored column order array had to be normalized", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const parsed = parsePrefs(JSON.stringify({ columnOrder: ["spark", "account", "bogus"] }));
    expect(parsed.columnOrder).toEqual(["spark", "account", "session", "week", "model", "updated"]);
    expect(warn).toHaveBeenCalledTimes(1);
    warn.mockRestore();
  });
  it("does not warn when a stored column order array is already normalized", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const order = [...DEFAULT_ORDER].reverse();
    parsePrefs(JSON.stringify({ columnOrder: order }));
    expect(warn).not.toHaveBeenCalled();
    warn.mockRestore();
  });
  it("defaults hiddenColumns and alwaysOnTop independently of each other", () => {
    const a = parsePrefs(JSON.stringify({ hiddenColumns: ["model", "account", "nope"], alwaysOnTop: "yes" }));
    expect(a.hiddenColumns).toEqual(["model"]);
    expect(a.alwaysOnTop).toBe(false);
    const b = parsePrefs(JSON.stringify({ hiddenColumns: "model", alwaysOnTop: true }));
    expect(b.hiddenColumns).toEqual([]);
    expect(b.alwaysOnTop).toBe(true);
  });
  it("warns when a stored hiddenColumns value is unusable", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    parsePrefs(JSON.stringify({ hiddenColumns: 42 }));
    expect(warn).toHaveBeenCalledTimes(1);
    warn.mockRestore();
  });
  it("never aliases the default hiddenColumns array", () => {
    const a = parsePrefs(null);
    a.hiddenColumns.push("session");
    expect(parsePrefs(null).hiddenColumns).toEqual([]);
  });
});

describe("loadPrefs / savePrefs", () => {
  it("round-trips through a store under the versioned key", () => {
    const store = new MemoryStore();
    const prefs = { font: "plex" as const, size: "lg" as const, columnOrder: [...DEFAULT_ORDER], hiddenColumns: ["updated" as const], alwaysOnTop: true };
    savePrefs(store, prefs);
    expect(store.data.has(PREFS_KEY)).toBe(true);
    expect(loadPrefs(store)).toEqual(prefs);
  });
  it("falls back to defaults without a store", () => {
    expect(loadPrefs(null)).toEqual(DEFAULT_PREFS);
  });
  it("warns instead of throwing when the store throws", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const broken = {
      getItem: (): string | null => { throw new Error("blocked"); },
      setItem: (): void => { throw new Error("blocked"); },
    };
    expect(loadPrefs(broken)).toEqual(DEFAULT_PREFS);
    expect(() => savePrefs(broken, DEFAULT_PREFS)).not.toThrow();
    expect(warn).toHaveBeenCalledTimes(2);
  });
});
