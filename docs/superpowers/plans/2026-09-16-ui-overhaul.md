# UI Overhaul Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the scaffold-grade dashboard with the "Usage Tracker" design from Claude Design (`docs/design/usage-tracker.dc.html` + `docs/design/usage-tracker-kit.js`), wired to the real Tauri backend.

**Architecture:** Pure, unit-tested TypeScript modules under `src/lib/` own every decision the design's kit made (thresholds, colours, column registry, series math, drag math, view prefs). Thin React components under `src/components/` render those decisions with CSS classes from a single token sheet (`src/styles.css`). One backend indirection (`src/lib/backend.ts`) lets the exact same UI run in a plain browser with a mock so the result can be screenshot-verified against the design.

**Tech Stack:** React 19, TypeScript 6 (strict, `noUnusedLocals`), Vite 8, Vitest 5 (node environment, `src/**/*.test.ts` only, no jsdom), Tauri 2, Rust (rusqlite). Fonts: `@fontsource/ibm-plex-sans`, `@fontsource/ibm-plex-mono`, `@fontsource/jetbrains-mono` (OFL-1.1).

**Spec:** Visual: `docs/design/usage-tracker.dc.html` (template) and `docs/design/usage-tracker-kit.js` (data/config layer). Behavioural contract still governed by `docs/2026-09-15-claude-usage-tracker-design.md` §7 (UI) and §8 (command surface). Where the two disagree, this plan's "Design deviations" section wins.

## Global Constraints

- Gates (all four must be green before any task is "done"): `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`, `npm test`, `npm run build`. Plain `cargo test`; never add `--test-threads=1`.
- TypeScript: no `any`, no non-null `!` assertions, no silent catches (every `catch` either surfaces via `onError` or `console.warn`s with context). `noUnusedLocals`/`noUnusedParameters` are on.
- Vitest runs in the **node** environment: tests may only import pure modules (`src/lib/*`). No React rendering in tests. Every interaction rule that needs a test is therefore a pure function in `src/lib/`.
- `import type { JSX } from "react"` for component return types (existing convention).
- Every async call that hits the backend goes through `call()`/`save()`-style wrappers with try/catch → `onError(message)`.
- New dependency installs use `socket npm install <pkg>` (global `ignore-scripts=true` stays). Fontsource packages are pure CSS+woff2; no scripts needed.
- CSP is `default-src 'self'; style-src 'self' 'unsafe-inline'` (`src-tauri/tauri.conf.json`). No external font/script URLs. Bundled fontsource assets are `'self'`.
- Dark theme only (the design is dark-only). Keep `color-scheme: dark` so native `<select>`/`<input>` render dark.
- Every timer, `window` listener and `listen()` subscription is removed on unmount / drag end.
- Commits: one per task, message prefix `feat(ui):`/`test(ui):`/`docs:` as fits, ending with the attribution trailer the session provides.

## Design deviations (decided 2026-09-16; do not re-litigate inside a task)

| Design element | Real-app behaviour | Reason |
|---|---|---|
| Edit drawer "Resets" row with day / time / tz editors | Read-only row: `session · resets in 2h 3m` and `weekly window · resets in 3d 4h` derived from `resets_at`. No editors. | Reset instants come from the CLI; there is no user-settable schedule in the backend. |
| Edit drawer "make default" button | Omitted. "move up" / "move down" buttons take that slot. | No backend command sets `is_default`; discovery owns it. Spec §7 requires keyboard reordering. |
| No Status column, no failure pill | `statusPill()` output rendered as a small pill inside the Account cell only when `tone !== "success"`; outcome pills with `outcome !== "ok"` are buttons that open `FailureDetail`. The account dot colour also reflects the tone, and an enabled account that has never been polled gets the idle (grey) dot, not the live one. | The design drops the column; the spec's failure surfacing must survive. The kit's dot formula has no never-polled state because its sample data is always polled. |
| Header "polling every 60 s" chip | Chip text/dot derived from `bannerFor()` (see Task 7). Error/warn banners (halt, stalled, no binary, no accounts) additionally render as a banner bar with their action button. | Chip alone cannot carry Clear halt / Open Settings. |
| Google Fonts `<link>` | Fontsource packages imported in `src/main.tsx`. | Offline desktop app, CSP `self`. |
| Font / text size / column order in component state | Persisted in `localStorage` under one key (`usage-tracker.prefs.v1`) via `src/lib/prefs.ts`. | Per-device view preferences; backend settings stay as they are. |
| "Last 30 days" drawer | `get_history` gains `days: Option<u32>` (1–30, default 7). Frontend asks for 30 once per cycle and derives the 7-day sparkline from the same points. | Backend returned 7 days only. |
| Per-model meter shows one model | Highest-pct model is the meter; note is its label, plus `· +N` when more exist; `title` lists all. | Row height is fixed at 66 px. |
| Poll gap / timeout save on every keystroke (current app) | Local draft, saved on blur or Enter. | Saving mid-typing trips `out_of_range`. |
| 7-day sparkline as connected polyline over daily values | Keeps the existing hourly `buildSparklinePaths` (gaps are path breaks), drawn in a `0 0 100 24` viewBox with `preserveAspectRatio="none"`. | Spec §7: missing hours are breaks, never zeros. Only the 30-day chart uses daily buckets. |

## File structure

Create:
- `src/lib/theme.ts` — colours, thresholds, `metricTone/metricColor`, `FONTS`, `SIZES` + key guards.
- `src/lib/columns.ts` — column registry, `DEFAULT_ORDER`, `gridTemplate`, `isColumnOrder`.
- `src/lib/prefs.ts` — `Prefs`, `parsePrefs`, `loadPrefs`, `savePrefs`.
- `src/lib/series.ts` — `dailyMax`, `polylinePoints`, `seriesDots`, `seriesStats`, `dayLabel`, `axisLabels`.
- `src/lib/drag.ts` — `rowDragTarget`, `rowShift`, `colDragTarget`, `colLineX`.
- `src/lib/present.ts` — `accountDotColor`, `summarizeModels`, `weekNote`, `sessionNote`, `accountCountLabel`, `chipFor`.
- `src/lib/backend.ts` — `backend()` accessor + `installMockBackendIfRequested()`.
- `src/lib/mockBackend.ts` — in-memory implementation for browser preview.
- `src/hooks/usePrefs.ts` — React hook over prefs.
- `src/components/Meter.tsx`, `src/components/AccountRow.tsx`, `src/components/HistoryDrawer.tsx`, `src/components/EditDrawer.tsx`, `src/components/Toggle.tsx`.
- Tests: `src/lib/theme.test.ts`, `src/lib/columns.test.ts`, `src/lib/prefs.test.ts`, `src/lib/series.test.ts`, `src/lib/drag.test.ts`, `src/lib/present.test.ts`.

Modify:
- `src/styles.css` (rewrite), `src/main.tsx`, `src/vite-env.d.ts`, `src/App.tsx`, `src/hooks/useDashboard.ts`, `src/components/Header.tsx` (rewrite), `src/components/AccountsTable.tsx` (rewrite), `src/components/Sparkline.tsx`, `src/components/Settings.tsx` (rewrite), `src/components/FailureDetail.tsx`, `src/lib/types.ts` (no shape changes; only if a task says so), `src-tauri/src/commands.rs` (`get_history`/`core_get_history`), `README.md`, `package.json`.

Delete: nothing. `src/lib/pill.ts`, `banner.ts`, `format.ts`, `sparkline.ts`, `reorder.ts` stay as they are (they are tested and still used).

---

### Task 1: Theme and column registry

**Files:**
- Create: `src/lib/theme.ts`, `src/lib/columns.ts`
- Test: `src/lib/theme.test.ts`, `src/lib/columns.test.ts`

**Interfaces:**
- Produces:
  - `THEME: { ok, warn, crit, live, idle, meta, metaWarn }` (hex strings)
  - `THRESHOLDS: { warn: 70, crit: 95 }`
  - `type MetricTone = "ok" | "warn" | "crit"`; `metricTone(pct: number): MetricTone`; `metricColor(pct: number): string`
  - `type FontKey = "system" | "plex" | "jetbrains"`; `FONTS: Record<FontKey, { label: string; ui: string; mono: string }>`; `FONT_KEYS: readonly FontKey[]`; `isFontKey(v: unknown): v is FontKey`
  - `type SizeKey = "sm" | "md" | "lg" | "xl"`; `SIZES: Record<SizeKey, { label: string; zoom: number }>`; `SIZE_KEYS: readonly SizeKey[]`; `isSizeKey(v: unknown): v is SizeKey`
  - `type ColumnKey = "account" | "session" | "week" | "model" | "spark" | "updated"`; `COLUMNS: Record<ColumnKey, { label: string; width: string }>`; `DEFAULT_ORDER: readonly ColumnKey[]`; `gridTemplate(order: readonly ColumnKey[]): string`; `isColumnOrder(v: unknown): v is ColumnKey[]`

- [ ] **Step 1: Write the failing tests**

`src/lib/theme.test.ts`:
```ts
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
```

`src/lib/columns.test.ts`:
```ts
import { describe, expect, it } from "vitest";
import { COLUMNS, DEFAULT_ORDER, gridTemplate, isColumnOrder } from "./columns";

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

describe("isColumnOrder", () => {
  it("accepts a permutation of every column exactly once", () => {
    expect(isColumnOrder([...DEFAULT_ORDER].reverse())).toBe(true);
  });
  it("rejects missing, duplicated or unknown keys and non-arrays", () => {
    expect(isColumnOrder(DEFAULT_ORDER.slice(1))).toBe(false);
    expect(isColumnOrder([...DEFAULT_ORDER, "account"])).toBe(false);
    expect(isColumnOrder(["account", "session", "week", "model", "spark", "cost"])).toBe(false);
    expect(isColumnOrder("account")).toBe(false);
    expect(isColumnOrder(null)).toBe(false);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm test -- src/lib/theme.test.ts src/lib/columns.test.ts`
Expected: FAIL — cannot resolve `./theme` / `./columns`.

- [ ] **Step 3: Implement**

`src/lib/theme.ts`:
```ts
/** Data colours from docs/design/usage-tracker-kit.js (THEME). */
export const THEME = {
  ok: "#7aa2f7",
  warn: "#d8a94f",
  crit: "#e0705f",
  live: "#5fb98a",
  idle: "#4b5359",
  meta: "#8b949e",
  metaWarn: "#d08c80",
} as const;

export const THRESHOLDS = { warn: 70, crit: 95 } as const;

export type MetricTone = "ok" | "warn" | "crit";

export function metricTone(pct: number): MetricTone {
  if (pct >= THRESHOLDS.crit) return "crit";
  if (pct >= THRESHOLDS.warn) return "warn";
  return "ok";
}

export function metricColor(pct: number): string {
  return THEME[metricTone(pct)];
}

export type FontKey = "system" | "plex" | "jetbrains";
export interface FontChoice { label: string; ui: string; mono: string }

export const FONTS: Record<FontKey, FontChoice> = {
  system: {
    label: "System",
    ui: "ui-sans-serif, 'Segoe UI', Helvetica, Arial, sans-serif",
    mono: "ui-monospace, 'Cascadia Mono', Consolas, 'SF Mono', monospace",
  },
  plex: {
    label: "IBM Plex",
    ui: "'IBM Plex Sans', Helvetica, sans-serif",
    mono: "'IBM Plex Mono', monospace",
  },
  jetbrains: {
    label: "JetBrains",
    ui: "ui-sans-serif, 'Segoe UI', Helvetica, sans-serif",
    mono: "'JetBrains Mono', monospace",
  },
};
export const FONT_KEYS: readonly FontKey[] = ["system", "plex", "jetbrains"];
export function isFontKey(v: unknown): v is FontKey {
  return typeof v === "string" && (FONT_KEYS as readonly string[]).includes(v);
}

export type SizeKey = "sm" | "md" | "lg" | "xl";
export interface SizeChoice { label: string; zoom: number }
export const SIZES: Record<SizeKey, SizeChoice> = {
  sm: { label: "small", zoom: 0.92 },
  md: { label: "default", zoom: 1 },
  lg: { label: "large", zoom: 1.1 },
  xl: { label: "largest", zoom: 1.22 },
};
export const SIZE_KEYS: readonly SizeKey[] = ["sm", "md", "lg", "xl"];
export function isSizeKey(v: unknown): v is SizeKey {
  return typeof v === "string" && (SIZE_KEYS as readonly string[]).includes(v);
}
```

`src/lib/columns.ts`:
```ts
/** Column registry from docs/design/usage-tracker-kit.js (COLUMNS). Cell
 *  rendering lives in AccountRow.tsx, keyed on ColumnKey. */
export type ColumnKey = "account" | "session" | "week" | "model" | "spark" | "updated";
export interface ColumnDef { label: string; width: string }

export const COLUMNS: Record<ColumnKey, ColumnDef> = {
  account: { label: "Account", width: "minmax(150px,1fr)" },
  session: { label: "Session", width: "minmax(86px,1.5fr)" },
  week: { label: "Week (all)", width: "minmax(86px,1.5fr)" },
  model: { label: "Per model", width: "minmax(86px,1.5fr)" },
  spark: { label: "7 days", width: "76px" },
  updated: { label: "Updated", width: "82px" },
};

export const DEFAULT_ORDER: readonly ColumnKey[] = [
  "account", "session", "week", "model", "spark", "updated",
];

/** Grip column first, per-row action column last. */
const LEAD_WIDTH = "26px";
const TRAIL_WIDTH = "62px";

export function gridTemplate(order: readonly ColumnKey[]): string {
  return [LEAD_WIDTH, ...order.map((k) => COLUMNS[k].width), TRAIL_WIDTH].join(" ");
}

export function isColumnOrder(v: unknown): v is ColumnKey[] {
  if (!Array.isArray(v) || v.length !== DEFAULT_ORDER.length) return false;
  const seen = new Set<string>();
  for (const item of v) {
    if (typeof item !== "string" || !(item in COLUMNS) || seen.has(item)) return false;
    seen.add(item);
  }
  return true;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test -- src/lib/theme.test.ts src/lib/columns.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/lib/theme.ts src/lib/theme.test.ts src/lib/columns.ts src/lib/columns.test.ts
git commit -m "feat(ui): theme thresholds, font/size choices and column registry"
```

---

### Task 2: View preferences (localStorage) and the `usePrefs` hook

**Files:**
- Create: `src/lib/prefs.ts`, `src/hooks/usePrefs.ts`
- Test: `src/lib/prefs.test.ts`

**Interfaces:**
- Consumes: `FontKey`, `SizeKey`, `isFontKey`, `isSizeKey` (theme.ts); `ColumnKey`, `DEFAULT_ORDER`, `isColumnOrder` (columns.ts).
- Produces:
  - `interface Prefs { font: FontKey; size: SizeKey; columnOrder: ColumnKey[] }`
  - `DEFAULT_PREFS: Prefs`, `PREFS_KEY = "usage-tracker.prefs.v1"`
  - `interface PrefsStore { getItem(key: string): string | null; setItem(key: string, value: string): void }`
  - `parsePrefs(raw: string | null): Prefs` — each field validated independently; anything invalid falls back to its default.
  - `loadPrefs(store: PrefsStore | null): Prefs` — `null` store or a throwing `getItem` → defaults (with `console.warn`).
  - `savePrefs(store: PrefsStore | null, prefs: Prefs): void` — throwing `setItem` → `console.warn`, never throws.
  - Hook: `usePrefs(): { prefs: Prefs; update: (patch: Partial<Prefs>) => void }`

- [ ] **Step 1: Write the failing tests**

`src/lib/prefs.test.ts`:
```ts
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
    expect(parsed).toEqual({ font: "plex", size: "md", columnOrder: [...DEFAULT_ORDER] });
  });
  it("accepts a full valid record", () => {
    const order = [...DEFAULT_ORDER].reverse();
    expect(parsePrefs(JSON.stringify({ font: "jetbrains", size: "xl", columnOrder: order })))
      .toEqual({ font: "jetbrains", size: "xl", columnOrder: order });
  });
  it("never aliases the default column order array", () => {
    const a = parsePrefs(null);
    a.columnOrder.reverse();
    expect(parsePrefs(null).columnOrder).toEqual([...DEFAULT_ORDER]);
  });
});

describe("loadPrefs / savePrefs", () => {
  it("round-trips through a store under the versioned key", () => {
    const store = new MemoryStore();
    const prefs = { font: "plex" as const, size: "lg" as const, columnOrder: [...DEFAULT_ORDER] };
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
```

- [ ] **Step 2: Run to verify failure**

Run: `npm test -- src/lib/prefs.test.ts` — Expected: FAIL, module not found.

- [ ] **Step 3: Implement**

`src/lib/prefs.ts`:
```ts
import { type ColumnKey, DEFAULT_ORDER, isColumnOrder } from "./columns";
import { type FontKey, type SizeKey, isFontKey, isSizeKey } from "./theme";

export interface Prefs {
  font: FontKey;
  size: SizeKey;
  columnOrder: ColumnKey[];
}

export const PREFS_KEY = "usage-tracker.prefs.v1";

export const DEFAULT_PREFS: Readonly<Prefs> = Object.freeze({
  font: "system",
  size: "md",
  columnOrder: Object.freeze([...DEFAULT_ORDER]) as unknown as ColumnKey[],
});

/** The subset of the Web Storage API the app touches; injectable for tests. */
export interface PrefsStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function fresh(): Prefs {
  return { font: DEFAULT_PREFS.font, size: DEFAULT_PREFS.size, columnOrder: [...DEFAULT_ORDER] };
}

/** Per-field validation: one bad field never discards the others. */
export function parsePrefs(raw: string | null): Prefs {
  const out = fresh();
  if (raw === null) return out;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return out;
  }
  if (typeof parsed !== "object" || parsed === null) return out;
  const rec = parsed as Record<string, unknown>;
  if (isFontKey(rec.font)) out.font = rec.font;
  if (isSizeKey(rec.size)) out.size = rec.size;
  if (isColumnOrder(rec.columnOrder)) out.columnOrder = [...rec.columnOrder];
  return out;
}

export function loadPrefs(store: PrefsStore | null): Prefs {
  if (store === null) return fresh();
  try {
    return parsePrefs(store.getItem(PREFS_KEY));
  } catch (e) {
    console.warn("prefs: could not read storage, using defaults", e);
    return fresh();
  }
}

export function savePrefs(store: PrefsStore | null, prefs: Prefs): void {
  if (store === null) return;
  try {
    store.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch (e) {
    console.warn("prefs: could not write storage", e);
  }
}
```
Note: the `catch {}` in `parsePrefs` is deliberate and documented by the function's contract (garbage → defaults); it is not a silent failure because the fallback is the specified behaviour. Keep the comment "Per-field validation…" above it.

`src/hooks/usePrefs.ts`:
```ts
import { useCallback, useState } from "react";
import { type Prefs, type PrefsStore, loadPrefs, savePrefs } from "../lib/prefs";

function storage(): PrefsStore | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch (e) {
    console.warn("prefs: localStorage unavailable", e);
    return null;
  }
}

export interface UsePrefs {
  prefs: Prefs;
  update: (patch: Partial<Prefs>) => void;
}

export function usePrefs(): UsePrefs {
  const [prefs, setPrefs] = useState<Prefs>(() => loadPrefs(storage()));
  const update = useCallback((patch: Partial<Prefs>): void => {
    setPrefs((prev) => {
      const next = { ...prev, ...patch };
      savePrefs(storage(), next);
      return next;
    });
  }, []);
  return { prefs, update };
}
```

- [ ] **Step 4: Run tests** — `npm test -- src/lib/prefs.test.ts` → PASS. Then `npx tsc --noEmit` → clean.

- [ ] **Step 5: Commit**

```bash
git add src/lib/prefs.ts src/lib/prefs.test.ts src/hooks/usePrefs.ts
git commit -m "feat(ui): persisted view preferences (font, text size, column order)"
```

---

### Task 3: Series helpers for the 30-day chart

**Files:**
- Create: `src/lib/series.ts`
- Test: `src/lib/series.test.ts`

**Interfaces:**
- Consumes: `HistoryPoint { t: number; pct: number }` (types.ts).
- Produces:
  - `dailyMax(points: readonly HistoryPoint[], days: number, now: number): Array<number | null>` — index `days-1` is the local calendar day containing `now`; each bucket is the max `pct` of that local day; `null` = no sample.
  - `polylinePoints(vals: readonly (number | null)[], width: number, height: number): string` — SVG `points` string through every non-null value (x evenly spaced by index over `[0,width]`, y = `height - clamp(v,0,100)/100*height`, 2 decimals); `""` when fewer than two known values.
  - `interface SeriesDot { index: number; value: number; leftPct: number }`; `seriesDots(vals): SeriesDot[]`
  - `interface SeriesStats { peak: number; avg: number; missing: number }`; `seriesStats(vals): SeriesStats` (avg rounded; zeros when nothing known).
  - `dayLabel(index: number, total: number, now: number, locale?: string): string` — e.g. `"Sep 16"`; index `total-1` is today.
  - `axisLabels(total: number, count: number, now: number, locale?: string): string[]` — `count` evenly spaced labels, first = oldest day, last = today.

- [ ] **Step 1: Write the failing tests**

`src/lib/series.test.ts`:
```ts
import { describe, expect, it } from "vitest";
import { axisLabels, dailyMax, dayLabel, polylinePoints, seriesDots, seriesStats } from "./series";

const HOUR = 3_600_000;
// A fixed local instant: 2026-09-16 15:00 local.
const NOW = new Date(2026, 8, 16, 15, 0, 0).getTime();
const dayStart = (offset: number): number => new Date(2026, 8, 16 + offset, 0, 0, 0).getTime();

describe("dailyMax", () => {
  it("puts today in the last bucket and takes the max per local day", () => {
    const out = dailyMax(
      [
        { t: dayStart(0) + 9 * HOUR, pct: 40 },
        { t: dayStart(0) + 13 * HOUR, pct: 46 },
        { t: dayStart(-1) + 22 * HOUR, pct: 12 },
      ],
      3,
      NOW,
    );
    expect(out).toEqual([null, 12, 46]);
  });
  it("ignores points outside the window and handles an empty series", () => {
    expect(dailyMax([{ t: dayStart(-5), pct: 99 }], 3, NOW)).toEqual([null, null, null]);
    expect(dailyMax([], 2, NOW)).toEqual([null, null]);
  });
  it("counts a point at 23:59 as its own day, not the next one", () => {
    const out = dailyMax([{ t: dayStart(0) - 60_000, pct: 7 }], 2, NOW);
    expect(out).toEqual([7, null]);
  });
});

describe("polylinePoints", () => {
  it("spaces known points by index and skips nulls without breaking the line", () => {
    expect(polylinePoints([0, null, 100], 100, 24)).toBe("0.00,24.00 100.00,0.00");
  });
  it("is empty with fewer than two known values", () => {
    expect(polylinePoints([50], 100, 24)).toBe("");
    expect(polylinePoints([null, null], 100, 24)).toBe("");
  });
  it("clamps values into 0..100", () => {
    expect(polylinePoints([150, -10], 100, 100)).toBe("0.00,0.00 100.00,100.00");
  });
});

describe("seriesDots / seriesStats", () => {
  it("emits one dot per known value with its left percentage", () => {
    expect(seriesDots([10, null, 30])).toEqual([
      { index: 0, value: 10, leftPct: 0 },
      { index: 2, value: 30, leftPct: 100 },
    ]);
    expect(seriesDots([5])).toEqual([{ index: 0, value: 5, leftPct: 0 }]);
  });
  it("computes peak, rounded average and missing count", () => {
    expect(seriesStats([10, null, 31])).toEqual({ peak: 31, avg: 21, missing: 1 });
    expect(seriesStats([null])).toEqual({ peak: 0, avg: 0, missing: 1 });
  });
});

describe("dayLabel / axisLabels", () => {
  it("labels today and earlier days", () => {
    expect(dayLabel(29, 30, NOW, "en-US")).toBe("Sep 16");
    expect(dayLabel(0, 30, NOW, "en-US")).toBe("Aug 18");
  });
  it("spreads count labels from oldest to today", () => {
    expect(axisLabels(30, 4, NOW, "en-US")).toEqual(["Aug 18", "Aug 28", "Sep 6", "Sep 16"]);
  });
});
```

- [ ] **Step 2: Run to verify failure** — `npm test -- src/lib/series.test.ts` → FAIL.

- [ ] **Step 3: Implement**

`src/lib/series.ts`:
```ts
import type { HistoryPoint } from "./types";

const DAY_MS = 86_400_000;

function startOfLocalDay(t: number): number {
  const d = new Date(t);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

/**
 * Daily buckets of the max hourly reading, last `days` local calendar days,
 * newest last. Rounding the day distance absorbs 23/25-hour DST days.
 */
export function dailyMax(
  points: readonly HistoryPoint[],
  days: number,
  now: number,
): Array<number | null> {
  const today = startOfLocalDay(now);
  const out: Array<number | null> = Array.from({ length: days }, () => null);
  for (const p of points) {
    const back = Math.round((today - startOfLocalDay(p.t)) / DAY_MS);
    const idx = days - 1 - back;
    if (idx < 0 || idx >= days) continue;
    const cur = out[idx];
    out[idx] = cur === null ? p.pct : Math.max(cur, p.pct);
  }
  return out;
}

/** Connected polyline through the known values (design: nulls are skipped). */
export function polylinePoints(
  vals: readonly (number | null)[],
  width: number,
  height: number,
): string {
  const n = vals.length;
  const pts: string[] = [];
  vals.forEach((v, i) => {
    if (v === null) return;
    const x = n === 1 ? 0 : (i / (n - 1)) * width;
    const y = height - (clamp(v, 0, 100) / 100) * height;
    pts.push(`${x.toFixed(2)},${y.toFixed(2)}`);
  });
  return pts.length > 1 ? pts.join(" ") : "";
}

export interface SeriesDot { index: number; value: number; leftPct: number }

export function seriesDots(vals: readonly (number | null)[]): SeriesDot[] {
  const n = vals.length;
  const out: SeriesDot[] = [];
  vals.forEach((v, i) => {
    if (v === null) return;
    out.push({ index: i, value: v, leftPct: n === 1 ? 0 : (i / (n - 1)) * 100 });
  });
  return out;
}

export interface SeriesStats { peak: number; avg: number; missing: number }

export function seriesStats(vals: readonly (number | null)[]): SeriesStats {
  const known = vals.filter((v): v is number => v !== null);
  const sum = known.reduce((a, b) => a + b, 0);
  return {
    peak: known.length ? Math.max(...known) : 0,
    avg: known.length ? Math.round(sum / known.length) : 0,
    missing: vals.length - known.length,
  };
}

export function dayLabel(index: number, total: number, now: number, locale?: string): string {
  const d = new Date(now);
  d.setDate(d.getDate() - (total - 1 - index));
  return d.toLocaleDateString(locale, { month: "short", day: "numeric" });
}

export function axisLabels(total: number, count: number, now: number, locale?: string): string[] {
  const c = Math.max(2, count);
  return Array.from({ length: c }, (_, i) =>
    dayLabel(Math.round((i * (total - 1)) / (c - 1)), total, now, locale),
  );
}
```

- [ ] **Step 4: Run tests** — PASS. `npx tsc --noEmit` clean.

- [ ] **Step 5: Commit**

```bash
git add src/lib/series.ts src/lib/series.test.ts
git commit -m "feat(ui): daily series helpers for the 30-day history chart"
```

---

### Task 4: Drag math

**Files:**
- Create: `src/lib/drag.ts`
- Test: `src/lib/drag.test.ts`

**Interfaces:**
- Produces:
  - `rowDragTarget(index: number, dy: number, rowH: number, length: number): number` — `clamp(index + round(dy / rowH), 0, length - 1)`.
  - `rowShift(i: number, dragIndex: number, target: number, rowH: number): number` — `-rowH` if `i > dragIndex && i <= target`; `rowH` if `i < dragIndex && i >= target`; else `0`.
  - `interface Rect { left: number; right: number; width: number }`
  - `colDragTarget(rects: readonly Rect[], clientX: number): number` — first index whose midpoint is right of `clientX`; last index when none; `0` for an empty list.
  - `colLineX(rects: readonly Rect[], target: number, index: number, wrapLeft: number): number` — `(target <= index ? r.left - 5 : r.right + 5) - wrapLeft`; `0` when `rects[target]` is missing.

- [ ] **Step 1: Write the failing tests**

`src/lib/drag.test.ts`:
```ts
import { describe, expect, it } from "vitest";
import { colDragTarget, colLineX, rowDragTarget, rowShift } from "./drag";

describe("rowDragTarget", () => {
  it("rounds the displacement to whole rows and clamps to the list", () => {
    expect(rowDragTarget(1, 0, 66, 3)).toBe(1);
    expect(rowDragTarget(1, 40, 66, 3)).toBe(2);
    expect(rowDragTarget(1, 32, 66, 3)).toBe(1);
    expect(rowDragTarget(1, -500, 66, 3)).toBe(0);
    expect(rowDragTarget(1, 500, 66, 3)).toBe(2);
  });
});

describe("rowShift", () => {
  it("moves rows between the origin and the target out of the way", () => {
    expect(rowShift(1, 0, 2, 66)).toBe(-66);
    expect(rowShift(2, 0, 2, 66)).toBe(-66);
    expect(rowShift(3, 0, 2, 66)).toBe(0);
    expect(rowShift(1, 2, 0, 66)).toBe(66);
    expect(rowShift(0, 2, 1, 66)).toBe(0);
    expect(rowShift(2, 2, 0, 66)).toBe(0);
  });
});

const rects = [
  { left: 0, right: 100, width: 100 },
  { left: 110, right: 210, width: 100 },
  { left: 220, right: 320, width: 100 },
];

describe("colDragTarget", () => {
  it("picks the first column whose midpoint is right of the pointer", () => {
    expect(colDragTarget(rects, 10)).toBe(0);
    expect(colDragTarget(rects, 60)).toBe(1);
    expect(colDragTarget(rects, 200)).toBe(2);
    expect(colDragTarget(rects, 999)).toBe(2);
    expect(colDragTarget([], 5)).toBe(0);
  });
});

describe("colLineX", () => {
  it("draws before the target when moving left, after it when moving right", () => {
    expect(colLineX(rects, 0, 2, 20)).toBe(-25);
    expect(colLineX(rects, 2, 0, 20)).toBe(305);
    expect(colLineX(rects, 1, 1, 0)).toBe(105);
    expect(colLineX(rects, 7, 0, 0)).toBe(0);
  });
});
```

- [ ] **Step 2: Run to verify failure** — FAIL, module not found.

- [ ] **Step 3: Implement**

`src/lib/drag.ts`:
```ts
/** Pointer-drag geometry for row and column reordering (pure, no DOM). */

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

export function rowDragTarget(index: number, dy: number, rowH: number, length: number): number {
  return clamp(index + Math.round(dy / rowH), 0, Math.max(0, length - 1));
}

export function rowShift(i: number, dragIndex: number, target: number, rowH: number): number {
  if (i > dragIndex && i <= target) return -rowH;
  if (i < dragIndex && i >= target) return rowH;
  return 0;
}

export interface Rect { left: number; right: number; width: number }

export function colDragTarget(rects: readonly Rect[], clientX: number): number {
  if (rects.length === 0) return 0;
  const idx = rects.findIndex((r) => clientX < r.left + r.width / 2);
  return idx < 0 ? rects.length - 1 : idx;
}

export function colLineX(rects: readonly Rect[], target: number, index: number, wrapLeft: number): number {
  const r = rects[target];
  if (r === undefined) return 0;
  return (target <= index ? r.left - 5 : r.right + 5) - wrapLeft;
}
```

- [ ] **Step 4: Run tests** — PASS.

- [ ] **Step 5: Commit**

```bash
git add src/lib/drag.ts src/lib/drag.test.ts
git commit -m "feat(ui): pure drag geometry for row and column reordering"
```

---

### Task 5: Presentation helpers (`present.ts`)

**Files:**
- Create: `src/lib/present.ts`
- Test: `src/lib/present.test.ts`

**Interfaces:**
- Consumes: `AccountRow`, `ModelWindow`, `Win`, `Dashboard` (types.ts); `Pill` (pill.ts); `Banner` + `bannerFor` (banner.ts); `THEME`, `THRESHOLDS` (theme.ts); `formatCountdown` (format.ts).
- Produces:
  - `accountDotColor(row: AccountRow, pill: Pill): string` — disabled → `THEME.idle`; pill kind `pending` (never polled) → `THEME.idle`; pill tone `error` → `THEME.crit`; tone `warn` → `THEME.warn`; else week_all pct ≥ crit → `THEME.crit`; else `THEME.live`. (The design kit has no "never polled" state because its sample data is always polled; the real app does, and green would claim a health it has not measured.)
  - `interface ModelSummary { pct: number; note: string; title: string }`; `summarizeModels(models: readonly ModelWindow[]): ModelSummary | null` — highest pct wins (first on ties); note `label` or `label · +N`; title `"Fable 47% · Opus 12%"`.
  - `weekNote(pct: number): { text: string; warn: boolean }` — `{ "at limit", true }` when ≥ crit, else `{ "all models", false }`.
  - `sessionNote(session: Win | null, now: number): string` — `"no data"` when null; `"idle"` when `resets_at === null`; else `formatCountdown(resets_at, now)`.
  - `accountCountLabel(n: number): string` — `"1 account"`, `"3 accounts"`.
  - `interface Chip { dot: "live" | "idle" | "warn" | "crit"; text: string }`; `chipFor(dashboard: Dashboard): Chip` — from `bannerFor`: halted → crit `"polling halted"`; stalled → warn `"stalled, recovered"`; no_binary → warn `"no claude binary"`; no_accounts → warn `"no enabled accounts"`; active → live `` `polling every ${interval_secs} s` ``; idle → idle `"idle · waits for Claude Code"`.

- [ ] **Step 1: Write the failing tests**

`src/lib/present.test.ts`:
```ts
import { describe, expect, it } from "vitest";
import type { Pill } from "./pill";
import { accountCountLabel, accountDotColor, chipFor, sessionNote, summarizeModels, weekNote } from "./present";
import { THEME } from "./theme";
import type { AccountRow, Dashboard } from "./types";

function row(over: Partial<AccountRow["account"]> = {}, weekPct: number | null = 10): AccountRow {
  return {
    account: {
      id: "a", label: "a", config_dir: "C:/a", enabled: true, disabled_reason: null,
      is_default: false, created_at: 0, sort_order: 0, ...over,
    },
    latest: weekPct === null ? null : {
      id: 1, account_id: "a", taken_at: 0, outcome: "ok", session: null,
      week_all: { pct: weekPct, resets_at: null }, week_models: [], error: null, duration_ms: 1,
    },
    backoff_until: null,
  };
}
const ok: Pill = { kind: "outcome", tone: "success", label: "ok" };

describe("accountDotColor", () => {
  it("is idle when disabled regardless of anything else", () => {
    expect(accountDotColor(row({ enabled: false }, 100), { kind: "disabled", tone: "neutral", label: "disabled" })).toBe(THEME.idle);
  });
  it("follows the pill tone for faults, then the weekly limit, then live", () => {
    expect(accountDotColor(row(), { kind: "outcome", tone: "error", label: "guard tripped" })).toBe(THEME.crit);
    expect(accountDotColor(row(), { kind: "backoff", tone: "warn", label: "backing off" })).toBe(THEME.warn);
    expect(accountDotColor(row({}, 96), ok)).toBe(THEME.crit);
    expect(accountDotColor(row({}, 50), ok)).toBe(THEME.live);
  });
  it("is idle while an enabled account has never been polled", () => {
    expect(accountDotColor(row({}, null), { kind: "pending", tone: "neutral", label: "not polled yet" })).toBe(THEME.idle);
  });
});

describe("summarizeModels", () => {
  it("is null with no models", () => {
    expect(summarizeModels([])).toBeNull();
  });
  it("shows the highest model and counts the rest", () => {
    expect(summarizeModels([{ label: "Fable", pct: 47, resets_at: null }])).toEqual({ pct: 47, note: "Fable", title: "Fable 47%" });
    expect(summarizeModels([
      { label: "Opus", pct: 12, resets_at: null },
      { label: "Fable", pct: 47, resets_at: null },
    ])).toEqual({ pct: 47, note: "Fable · +1", title: "Opus 12% · Fable 47%" });
  });
});

describe("notes and labels", () => {
  it("weekNote flags the limit", () => {
    expect(weekNote(95)).toEqual({ text: "at limit", warn: true });
    expect(weekNote(94)).toEqual({ text: "all models", warn: false });
  });
  it("sessionNote covers missing, idle and counting-down sessions", () => {
    expect(sessionNote(null, 0)).toBe("no data");
    expect(sessionNote({ pct: 0, resets_at: null }, 0)).toBe("idle");
    expect(sessionNote({ pct: 40, resets_at: 3_600_000 * 2 + 60_000 * 3 }, 0)).toBe("resets in 2h 3m");
  });
  it("accountCountLabel pluralises", () => {
    expect(accountCountLabel(1)).toBe("1 account");
    expect(accountCountLabel(0)).toBe("0 accounts");
    expect(accountCountLabel(3)).toBe("3 accounts");
  });
});

describe("chipFor", () => {
  const base: Dashboard = {
    accounts: [row()], gate: "active", busy: false, halted: null, stalled_at: null,
    binary: { path: "C:/claude.exe", source: "local_bin" }, interval_secs: 60,
  };
  it("maps each banner kind to a dot and short text", () => {
    expect(chipFor(base)).toEqual({ dot: "live", text: "polling every 60 s" });
    expect(chipFor({ ...base, gate: "idle" })).toEqual({ dot: "idle", text: "idle · waits for Claude Code" });
    expect(chipFor({ ...base, halted: "guard" })).toEqual({ dot: "crit", text: "polling halted" });
    expect(chipFor({ ...base, stalled_at: 1 })).toEqual({ dot: "warn", text: "stalled, recovered" });
    expect(chipFor({ ...base, binary: { path: null, source: null } })).toEqual({ dot: "warn", text: "no claude binary" });
    expect(chipFor({ ...base, accounts: [row({ enabled: false })] })).toEqual({ dot: "warn", text: "no enabled accounts" });
  });
});
```

- [ ] **Step 2: Run to verify failure** — FAIL.

- [ ] **Step 3: Implement**

`src/lib/present.ts`:
```ts
import { bannerFor } from "./banner";
import { formatCountdown } from "./format";
import type { Pill } from "./pill";
import { THEME, THRESHOLDS } from "./theme";
import type { AccountRow, Dashboard, ModelWindow, Win } from "./types";

/** Account dot: disabled > never polled > fault tone > weekly limit > live. */
export function accountDotColor(row: AccountRow, pill: Pill): string {
  if (!row.account.enabled || pill.kind === "pending") return THEME.idle;
  if (pill.tone === "error") return THEME.crit;
  if (pill.tone === "warn") return THEME.warn;
  const week = row.latest?.week_all?.pct ?? 0;
  return week >= THRESHOLDS.crit ? THEME.crit : THEME.live;
}

export interface ModelSummary { pct: number; note: string; title: string }

export function summarizeModels(models: readonly ModelWindow[]): ModelSummary | null {
  if (models.length === 0) return null;
  let top = models[0];
  for (const m of models) if (m.pct > top.pct) top = m;
  const rest = models.length - 1;
  return {
    pct: top.pct,
    note: rest > 0 ? `${top.label} · +${rest}` : top.label,
    title: models.map((m) => `${m.label} ${m.pct}%`).join(" · "),
  };
}

export function weekNote(pct: number): { text: string; warn: boolean } {
  return pct >= THRESHOLDS.crit ? { text: "at limit", warn: true } : { text: "all models", warn: false };
}

export function sessionNote(session: Win | null, now: number): string {
  if (session === null) return "no data";
  if (session.resets_at === null) return "idle";
  return formatCountdown(session.resets_at, now);
}

export function accountCountLabel(n: number): string {
  return `${n} ${n === 1 ? "account" : "accounts"}`;
}

export interface Chip { dot: "live" | "idle" | "warn" | "crit"; text: string }

export function chipFor(dashboard: Dashboard): Chip {
  const banner = bannerFor(dashboard);
  switch (banner?.kind) {
    case "halted": return { dot: "crit", text: "polling halted" };
    case "stalled": return { dot: "warn", text: "stalled, recovered" };
    case "no_binary": return { dot: "warn", text: "no claude binary" };
    case "no_accounts": return { dot: "warn", text: "no enabled accounts" };
    case "active": return { dot: "live", text: `polling every ${dashboard.interval_secs} s` };
    default: return { dot: "idle", text: "idle · waits for Claude Code" };
  }
}
```

- [ ] **Step 4: Run tests** — PASS. `npx tsc --noEmit` clean.

- [ ] **Step 5: Commit**

```bash
git add src/lib/present.ts src/lib/present.test.ts
git commit -m "feat(ui): presentation helpers for dot colour, model summary, notes and chip"
```

---

### Task 6: 30-day history from the backend, backend indirection and browser mock

**Files:**
- Modify: `src-tauri/src/commands.rs` (`core_get_history` ~line 145, `get_history` ~line 339, test caller ~line 1005)
- Create: `src/lib/backend.ts`, `src/lib/mockBackend.ts`
- Modify: `src/hooks/useDashboard.ts`, `src/main.tsx`, `src/vite-env.d.ts`
- Test: Rust unit test in `commands.rs` `mod tests`

**Interfaces:**
- Produces (Rust): `pub fn core_get_history(core: &Core, account_id: &str, now: i64, days: u32) -> AppResult<Vec<HistoryPoint>>` (days clamped to `1..=MAX_HISTORY_DAYS` where `pub const MAX_HISTORY_DAYS: u32 = 30`); command `get_history(account_id: String, days: Option<u32>)` — `None` → 7.
- Produces (TS):
  - `interface Backend { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>; listen(event: string, handler: () => void): Promise<() => void> }`
  - `backend(): Backend`; `installMockBackendIfRequested(): Promise<void>` (reads `import.meta.env.VITE_MOCK_BACKEND === "1"`).
  - `createMockBackend(): Backend` in `mockBackend.ts`.
  - `HISTORY_DAYS = 30` exported from `useDashboard.ts`; history requested with `{ accountId, days: HISTORY_DAYS }`.

- [ ] **Step 1: Rust — write the failing test**

In `src-tauri/src/commands.rs` `mod tests`, directly after `get_history_returns_hourly_points` (line 986–1008), add. It uses the module's existing fixtures exactly as that test does: `core() -> (tempfile::TempDir, Arc<Core>)`, `make_dir(&Path, &str)`, `core_add_account(&Core, &Path, now: i64)`, and `Store::insert_snapshot(&self, account_id: &str, taken_at: i64, outcome: &PollOutcome, raw: Option<&str>, duration_ms: u32)`:
```rust
#[test]
fn history_days_is_clamped_to_one_through_thirty() {
    let (tmp, core) = core();
    let d = make_dir(tmp.path(), ".claude3");
    let a = core_add_account(&core, &d, 1).expect("add");
    let hour = 3_600_000i64;
    let day = 24 * hour;
    let base = 1_000 * day;
    // One ok snapshot per listed day, each in its own hourly bucket.
    for (days_ago_from_base, pct) in [(0i64, 1u8), (10, 2), (29, 3), (31, 4)] {
        let outcome = PollOutcome::Ok(crate::usage::Parsed {
            session: crate::usage::Window { pct: 1, resets_at: None },
            week_all: crate::usage::Window { pct, resets_at: None },
            week_models: vec![],
        });
        core.store
            .insert_snapshot(&a.id, base + days_ago_from_base * day, &outcome, None, 1)
            .expect("snapshot");
    }
    // `now` is one hour past the newest snapshot (day 31), so "N days" reaches
    // back to day 31 - N + 1/24: 1 day → {31}; 7 → {29, 31}; 30 → {10, 29, 31}.
    let now = base + 31 * day + hour;
    assert_eq!(core_get_history(&core, &a.id, now, 0).expect("h").len(), 1, "0 clamps to 1 day");
    assert_eq!(core_get_history(&core, &a.id, now, 7).expect("h").len(), 2, "7 days: 29, 31");
    assert_eq!(core_get_history(&core, &a.id, now, 30).expect("h").len(), 3, "30 days: 10, 29, 31");
    assert_eq!(core_get_history(&core, &a.id, now, 999).expect("h").len(), 3, "999 clamps to 30");
}
```
Also change the existing call in `get_history_returns_hourly_points` to `core_get_history(&core, &a.id, base + hour * 8, 7)`.

- [ ] **Step 2: Run** `cargo test --manifest-path src-tauri/Cargo.toml history_days_is_clamped` → FAIL (wrong arity).

- [ ] **Step 3: Implement in Rust**

```rust
/// Longest history the UI can ask for; matches store retention (D10, 30 days).
pub const MAX_HISTORY_DAYS: u32 = 30;
const DEFAULT_HISTORY_DAYS: u32 = 7;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

pub fn core_get_history(core: &Core, account_id: &str, now: i64, days: u32) -> AppResult<Vec<HistoryPoint>> {
    let days = i64::from(days.clamp(1, MAX_HISTORY_DAYS));
    core.store.history(account_id, now - days * DAY_MS)
}

#[tauri::command]
pub async fn get_history(
    core: State<'_, SharedCore>,
    account_id: String,
    days: Option<u32>,
) -> AppResult<Vec<HistoryPoint>> {
    let core = Arc::clone(&core);
    let days = days.unwrap_or(DEFAULT_HISTORY_DAYS);
    blocking(move || core_get_history(&core, &account_id, now_ms(), days)).await
}
```
Update the existing test caller at ~line 1005 to pass `7`. Add a `debug!(account_id, days, "history requested")` if the surrounding commands log at debug; follow the file's existing logging pattern.

- [ ] **Step 4: Run Rust gates** — `cargo test --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` → green.

- [ ] **Step 5: TS backend indirection**

`src/vite-env.d.ts` — append:
```ts
interface ImportMetaEnv {
  readonly VITE_MOCK_BACKEND?: string;
}
interface ImportMeta {
  readonly env: ImportMetaEnv;
}
```

`src/lib/backend.ts`:
```ts
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";

/** The two backend primitives the UI uses. Swappable for a browser mock. */
export interface Backend {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: () => void): Promise<() => void>;
}

const real: Backend = {
  invoke: <T>(command: string, args?: Record<string, unknown>): Promise<T> =>
    tauriInvoke<T>(command, args),
  listen: async (event, handler) => {
    const off = await tauriListen(event, () => handler());
    return () => off();
  },
};

let current: Backend = real;

export function backend(): Backend {
  return current;
}

/** `VITE_MOCK_BACKEND=1 npm run dev` renders the UI in a plain browser. */
export async function installMockBackendIfRequested(): Promise<void> {
  if (import.meta.env.VITE_MOCK_BACKEND !== "1") return;
  const { createMockBackend } = await import("./mockBackend");
  current = createMockBackend();
  console.warn("usage tracker: mock backend active (VITE_MOCK_BACKEND=1)");
}
```

`src/lib/mockBackend.ts` — in-memory state seeded from the design's sample accounts translated to the real DTOs. Requirements:
- Three accounts: `claude3` (default, session 40 % resetting in 2 h 3 m, week 46, model `Fable 47%`), `claude` (week 100, model `Fable 68%`, session idle, backoff null), `claude2` (week 98, model `Fable 99%`, plus a second model `Opus 12%`), `taken_at` 53–59 s before `Date.now()`. Give `claude2` `outcome: "timeout"` with `error: "claude exited after 30 s"` so a pill is visible.
- History: for each account build hourly `HistoryPoint`s from the kit's 30-value `days` arrays: for day index `i` with value `v` (skip `null`) emit points at 10:00, 13:00 and 16:00 local of that day with pct `v-8`, `v`, `v-3` clamped to ≥ 0. Day index 29 = today.
- Dashboard: `gate: "active"`, `busy: false`, `halted: null`, `stalled_at: null`, `binary: { path: "C:\\Users\\josh\\.local\\bin\\claude.exe", source: "local_bin" }`, `interval_secs: 60`.
- Settings: `{ interval_secs: 60, timeout_secs: 30, claude_binary: "", close_to_tray: true, launch_at_login: false, log_level: "info" }`.
- `invoke` implements: `get_dashboard`, `get_history` (respect `days` by filtering `t >= now - days*DAY`), `poll_now` (sets every `taken_at = Date.now()`, returns `"started"`), `add_account` (push a disabled account with `label` = last path segment; throw `{ code: "duplicate", message }` if the path exists), `update_account` (`label`/`enabled`), `remove_account`, `reorder_accounts`, `rescan_profiles` (returns `[]`), `get_settings`, `set_settings` (throw `{ code: "out_of_range", message: "interval_secs must be 10..=3600" }` when out of range, mirroring the backend), `clear_halt`, `open_login`, `open_log_dir`, `get_snapshot_raw` (returns `{ raw: "Session: 40% …", error }`). Unknown command → `throw { code: "internal", message: \`mock: unknown command ${command}\` }`.
- `listen` returns `Promise.resolve(() => undefined)`.
- Every mutation logs `console.debug("mock", command, args)`.
- Type it strictly: build a `Record<string, (args: Record<string, unknown>) => unknown>` and cast the result with `as T` once in `invoke` — that is the one place a cast is allowed, and it is commented as the mock's boundary.

`src/hooks/useDashboard.ts` — replace the two `@tauri-apps/api` imports with `import { backend } from "../lib/backend";`, use `backend().invoke` / `backend().listen`, and:
```ts
export const HISTORY_DAYS = 30;
...
const points = await backend().invoke<HistoryPoint[]>("get_history", {
  accountId: row.account.id,
  days: HISTORY_DAYS,
});
```
`listen` handlers already ignore the payload; `off()` calls stay as they are (`Backend.listen` returns `() => void`).

`src/main.tsx`:
```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { installMockBackendIfRequested } from "./lib/backend";

const container = document.getElementById("root");
if (container === null) {
  throw new Error("missing #root element");
}
const root = ReactDOM.createRoot(container);

installMockBackendIfRequested()
  .catch((e: unknown) => console.warn("mock backend failed to load", e))
  .finally(() => {
    root.render(
      <React.StrictMode>
        <App />
      </React.StrictMode>,
    );
  });
```

- [ ] **Step 6: Verify** — `npm test`, `npm run build` green. Then run `VITE_MOCK_BACKEND=1 npx vite --port 1420` (PowerShell: `$env:VITE_MOCK_BACKEND='1'; npx vite --port 1420`) in the background and open `http://localhost:1420` with the Playwright MCP (`browser_navigate`, `browser_snapshot`): the *old* UI must render the three mock accounts. Stop the server.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands.rs src/lib/backend.ts src/lib/mockBackend.ts src/hooks/useDashboard.ts src/main.tsx src/vite-env.d.ts
git commit -m "feat: get_history takes a day count; backend indirection with a browser mock"
```

---

### Task 7: Tokens, fonts, app shell and header

**Files:**
- Modify: `src/styles.css` (full rewrite), `src/main.tsx` (font imports), `src/App.tsx`, `src/components/Header.tsx` (rewrite), `package.json`/`package-lock.json` (fontsource deps)

**Interfaces:**
- Consumes: `usePrefs`, `FONTS`, `SIZES`, `chipFor`, `accountCountLabel`, `bannerFor`.
- Produces: CSS classes listed below (later tasks use these exact names); `Header` props `{ dashboard: Dashboard; settingsOpen: boolean; onToggleSettings: () => void; onChanged: () => void; onError: (m: string) => void }`.

- [ ] **Step 1: Install fonts**

```bash
socket npm install @fontsource/ibm-plex-sans@5 @fontsource/ibm-plex-mono@5 @fontsource/jetbrains-mono@5
```
Confirm `package-lock.json` gained exactly those three packages and nothing else changed. In `src/main.tsx` add at the top:
```ts
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
```

- [ ] **Step 2: Rewrite `src/styles.css`**

Tokens (`:root`), all from the design:
```css
:root {
  color-scheme: dark;
  --bg: #0b0d0e;            /* page */
  --panel: #101315;         /* table + settings cards */
  --panel-head: #0d1012;    /* column header strip */
  --drawer: #0d1113;        /* chart / edit drawers */
  --chart: #0f1416;         /* chart plot area */
  --line: #1e2327;          /* card border */
  --line-soft: #191e21;     /* row dividers */
  --line-head: #1c2125;     /* header strip border, chart border, dividers */
  --fg: #e7eaec;
  --fg-strong: #eef1f3;
  --fg-dim: #79828a;        /* disabled account name */
  --muted: #8b949e;
  --muted-2: #9aa3aa;       /* uppercase labels */
  --dim: #5a6369;           /* the "·" separators */
  --btn-bg: #181c1f;
  --btn-border: #262d33;
  --btn-fg: #cfd6db;
  --btn-hover: #1f252a;
  --accent: #8ab4f8;
  --accent-bg: #151c24;
  --input-bg: #15191c;
  --track-off: #1e2429;
  --track-on: #3f6fb5;
  --knob-off: #6b747b;
  --knob-on: #eaf1fb;
  --danger: #e58e84;
  --danger-border: #3a2a28;
  --danger-hover-bg: #241a19;
  --tag-fg: #9fb6d3; --tag-border: #2c3a49; --tag-bg: #161d24;
  --ok: #7aa2f7; --warn: #d8a94f; --crit: #e0705f; --live: #5fb98a; --idle: #4b5359;
  --ui: ui-sans-serif, 'Segoe UI', Helvetica, Arial, sans-serif;
  --mono: ui-monospace, 'Cascadia Mono', Consolas, 'SF Mono', monospace;
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg); font-family: var(--ui); font-size: 14px; }
input, select, button { font-family: inherit; }
```
Class contract (implement each with the exact pixel values from `docs/design/usage-tracker.dc.html`; the implementer reads the design for values not restated here):
- Shell: `.app` (min-height 100vh; padding 28px 22px 48px; flex; center), `.app-inner` (width 100%; max-width 1120px; column; gap 14px).
- Header: `.topbar`, `.topbar-title` (19px/600/-0.01em), `.topbar-count` (mono 12px muted), `.topbar-actions`, `.chip`, `.chip-dot` + modifiers `.chip-dot-live|idle|warn|crit`, `.chip-text`.
- Buttons: `.btn` (mono 12px, `--btn-*`, radius 7px, padding 7px 13px, hover `--btn-hover` + `#fff`), `.btn-sm` (11.5px, padding 6px 11px), `.btn-primary` (bg `--accent`, fg `--bg`, no border), `.btn-ghost` (transparent bg, `--muted-2` fg, border `--btn-border`), `.btn-danger` (transparent, `--danger` fg, `--danger-border`, hover `--danger-hover-bg` / `#f3a79d`), `.btn-link` (no border/bg, muted, hover `#dde2e6`), `:disabled` → opacity .5, cursor default.
- Banner: `.banner`, `.banner-error`, `.banner-warn` (same look as today but on the new tokens; radius 10px; mono 12px).
- Table: `.panel` (border `--line`, radius 14px, bg `--panel`, overflow hidden), `.thead-wrap` (relative), `.thead` (grid; gap 10px; padding 11px 16px; border-bottom `--line-head`; bg `--panel-head`), `.th` (mono 11px, .08em, uppercase, ellipsis, cursor grab, user-select none, padding 3px 5px, margin -3px -5px, radius 5px, colour `--muted-2`, hover `#dde2e6` on `#191e22`), `.th-dragging` (colour #fff, bg #232b32, z 6, no transition), `.th-dimmed` (colour #767f86), `.col-line` (absolute 2px accent line), `.rows` (relative), `.row-placeholder` (absolute dashed `#3a4a57` radius 10px bg `rgba(122,162,247,.05)`), `.row` (relative; bg `--panel`; border-bottom `--line-soft`; transition transform/box-shadow 150ms), `.row-lifted` (no transition; shadow `0 18px 38px rgba(0,0,0,.55)`; radius 10px; z 20), `.row-parked` (opacity .72), `.row-grid` (grid; gap 10px; padding 14px 16px; height 66px; align center), `.grip` (16px wide, 6 dots 3px `#626b72`, cursor grab, hover bg `#1b2024`), `.cell` (min-width 0; radius 6px), `.cell-hot` (bg `rgba(122,162,247,.07)`).
- Account cell: `.acct`, `.acct-dot` (6px), `.acct-name` (14px/500; ellipsis), `.acct-name-off` (`--fg-dim`), `.tag` (mono 10px uppercase `--tag-*`, radius 4px, padding 2px 5px).
- Status pill in the account cell: `.pill`, `.pill-tone-success|warn|error|neutral`, `.pill-backoff`, `.pill-pending`, `.pill-disabled` — same semantics as today, restyled: mono 10px, radius 4px, padding 2px 5px, border 1px, colours warn→`--warn`, error→`--crit` (+ bg `rgba(224,112,95,.12)`), neutral→`--muted`; `button.pill` gets `cursor: pointer`.
- Meter: `.meter`, `.meter-track` (5px, radius 3px, bg `#1e2429`), `.meter-fill`, `.meter-meta` (mono 11px), `.meter-pct` (12px, width 38px, `#dbe1e6`, tabular-nums), `.meter-sep` (`--dim`), `.meter-note` (ellipsis), `.meter-note-warn` (`#d08c80`).
- Spark: `.spark` (button reset; 32px tall; flex; radius 6px; padding 3px 5px; border 1px `--line-head`; hover border `#33404b` bg `#161b1f`), `.spark-open` (same as hover), `.spark svg` (width 100%; height 24px; overflow visible), `.spark-empty` (muted em dash).
- Updated cell: `.updated` (mono 11.5px muted; ellipsis; tabular-nums).
- Row actions: `.row-actions` (flex end), `.btn-edit-on` (accent bg, `--bg` fg).
- Drawers: `.drawer` (padding 16px 18px 18px 56px; bg `--drawer`; border-top `--line-soft`; column; gap 10px), `.drawer-head` (mono 11px; space-between; wrap), `.drawer-label` (uppercase .07em `--muted-2`), `.drawer-stats` (flex; gap 14px; muted), `.chart` (relative; height 148px; border `--line-head`; radius 10px; bg `--chart`; padding 14px 16px), `.chart svg` (100%/100%; overflow visible), `.chart-dots` (absolute; inset 14px 16px; pointer-events none), `.dot` (absolute; 8px; radius 50%; bg `#e9eef3`; border 2px `--chart`; shadow; translate(-50%,50%); pointer-events auto; hover scale 1.55 + `#fff`), `.tip` (absolute; z 8; mono 11px; `#eef3f8` on `#1d2429`; border `#333d45`; radius 7px; padding 5px 8px; shadow), `.chart-y` (absolute mono 10px muted; `.chart-y-top` top 8px, `.chart-y-bottom` bottom 6px; left -4px), `.chart-axis` (flex space-between; mono 10px muted).
- Edit drawer: `.edit-row` (flex; gap 10px; center; wrap), `.edit-label` (mono 11px uppercase `--muted-2`; width 78px), `.input` (bg `--input-bg`; border `--btn-border`; radius 7px; padding 8px 10px; fg; 13px; outline none; focus border `--accent`), `.input-mono` (mono 12px), `.edit-hint` (mono 11px muted), `.edit-path` (mono 11px muted; ellipsis; user-select all), `.ml-auto`.
- Settings: `.settings` (`.panel` + padding 22px 24px; column; gap 20px), `.settings-head`, `.settings-title` (15px/600), `.section` (column; gap 10px), `.section-label` (mono 11px uppercase .07em `--muted-2`), `.choices` (flex; gap 8px; wrap), `.choice` (column; gap 3px; padding 9px 13px; radius 8px; border `--btn-border`; bg `--input-bg`; fg `#c3cad0`; hover border `#3a464f`), `.choice-on` (border `--accent`; bg `--accent-bg`; fg `#eef3f8`), `.choice-sample` (13px), `.choice-sub` (mono 11px muted), `.choice-size` (mono 11.5px; padding 8px 14px), `.divider` (1px `--line-head`), `.settings-grid` (grid auto-fit minmax(220px,1fr); gap 16px), `.field` (column; gap 7px), `.field-row` (flex; gap 8px; center), `.input-num` (width 90px; mono 12.5px), `.toggles`, `.toggle-row` (space-between; gap 16px; padding 11px 2px; cursor pointer; border-bottom `#171b1e`; button reset; text-align left; width 100%), `.toggle-text` (13px `#dde2e6`), `.toggle-hint` (mono 11px muted), `.switch` (36×20; radius 999px; padding 2px; bg `--track-off`; transition background 160ms), `.switch-on` (bg `--track-on`), `.knob` (16px; radius 50%; bg `--knob-off`; transition transform 160ms), `.switch-on .knob` (translateX(16px); bg `--knob-on`), `.accounts-row` (flex; gap 9px; wrap; center), `.input-grow` (flex 1; min-width 220px).
- Footer hint: `.hint` (mono 11px muted).
- Modal + toast: `.modal` (fixed inset 0; `rgba(0,0,0,.6)`; grid center), `.modal-body` (bg `--panel`; border `--line`; radius 14px; padding 20px 22px; max-width 720px; max-height 80vh; overflow auto), `.modal-body pre` (bg `--input-bg`; border `--btn-border`; radius 8px; padding 10px; mono 12px; pre-wrap), `.raw` (max-height 320px; overflow auto), `.toast` (fixed right/bottom 16px; bg `#241a19`; border `--danger-border`; fg `#f3a79d`; mono 12px; radius 10px; padding 10px 14px), `.error` (`--danger`).
- Loading state: `.loading` (mono 12px muted; padding 28px).

- [ ] **Step 3: App shell**

`src/App.tsx`:
```tsx
import type { CSSProperties, JSX } from "react";
import { useState } from "react";
import { AccountsTable } from "./components/AccountsTable";
import { FailureDetail } from "./components/FailureDetail";
import { Header } from "./components/Header";
import { Settings } from "./components/Settings";
import { useDashboard } from "./hooks/useDashboard";
import { usePrefs } from "./hooks/usePrefs";
import { FONTS, SIZES } from "./lib/theme";
import "./styles.css";

export default function App(): JSX.Element {
  const { dashboard, history, now, error, refetch } = useDashboard();
  const { prefs } = usePrefs(); // `update` joins in Task 8
  const [showSettings, setShowSettings] = useState(false);
  const [failureId, setFailureId] = useState<number | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const showError = (message: string): void => {
    setToast(message);
    window.setTimeout(() => setToast(null), 6000);
  };

  const font = FONTS[prefs.font];
  // Custom properties are not in CSSProperties; this is the one typed seam.
  const shellStyle = {
    "--ui": font.ui,
    "--mono": font.mono,
    zoom: SIZES[prefs.size].zoom,
  } as CSSProperties;

  if (dashboard === null) {
    return (
      <main className="app" style={shellStyle}>
        <div className="app-inner"><p className="loading">{error ?? "loading…"}</p></div>
      </main>
    );
  }

  return (
    <main className="app" style={shellStyle}>
      <div className="app-inner">
        <Header
          dashboard={dashboard}
          settingsOpen={showSettings}
          onToggleSettings={() => setShowSettings((v) => !v)}
          onChanged={refetch}
          onError={showError}
        />
        {/* Task 7 state: AccountsTable and Settings keep their CURRENT props.
            Task 8 replaces this AccountsTable call, Task 11 the Settings call. */}
        <AccountsTable
          rows={dashboard.accounts}
          history={history}
          now={now}
          onChanged={refetch}
          onError={showError}
          onShowFailure={(id) => setFailureId(id)}
        />
        <p className="hint">drag a row handle to set failover priority · drag a column header to reorder columns</p>
        {showSettings && (
          <Settings
            binary={dashboard.binary}
            onClose={() => setShowSettings(false)}
            onChanged={refetch}
            onError={showError}
          />
        )}
        {failureId !== null && <FailureDetail snapshotId={failureId} onClose={() => setFailureId(null)} />}
        {toast !== null && <div className="toast" role="status">{toast}</div>}
      </div>
    </main>
  );
}
```
The toast timer: keep the existing behaviour (a `window.setTimeout` per toast) but store the id in a `useRef<number | null>` and clear it in a `useEffect` cleanup so an unmount never fires a stale `setToast`:
```tsx
const toastTimer = useRef<number | null>(null);
const showError = (message: string): void => {
  if (toastTimer.current !== null) window.clearTimeout(toastTimer.current);
  setToast(message);
  toastTimer.current = window.setTimeout(() => setToast(null), 6000);
};
useEffect(() => () => { if (toastTimer.current !== null) window.clearTimeout(toastTimer.current); }, []);
```
(add `useEffect`, `useRef` to the react import.)

This file compiles at the Task 7 commit because `AccountsTable` and `Settings` are called with exactly their current props.

- [ ] **Step 4: Header**

`src/components/Header.tsx`:
```tsx
import type { JSX } from "react";
import { backend } from "../lib/backend";
import { bannerFor } from "../lib/banner";
import { accountCountLabel, chipFor } from "../lib/present";
import type { Dashboard } from "../lib/types";

interface Props {
  dashboard: Dashboard;
  settingsOpen: boolean;
  onToggleSettings: () => void;
  onChanged: () => void;
  onError: (message: string) => void;
}

export function Header({ dashboard, settingsOpen, onToggleSettings, onChanged, onError }: Props): JSX.Element {
  const banner = bannerFor(dashboard);
  const chip = chipFor(dashboard);

  const run = async (command: string): Promise<void> => {
    try {
      await backend().invoke(command);
      onChanged();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <header className="header">
      <div className="topbar">
        <div className="topbar-left">
          <h1 className="topbar-title">Usage Tracker</h1>
          <span className="topbar-count">{accountCountLabel(dashboard.accounts.length)}</span>
        </div>
        <div className="topbar-actions">
          <div className="chip" title={banner?.text}>
            <span className={`chip-dot chip-dot-${chip.dot}`} />
            <span className="chip-text">{chip.text}</span>
          </div>
          <button type="button" className="btn" onClick={() => void run("poll_now")} disabled={dashboard.busy}>
            {dashboard.busy ? "refreshing…" : "refresh"}
          </button>
          <button type="button" className={`btn${settingsOpen ? " btn-edit-on" : ""}`} onClick={onToggleSettings}>
            settings
          </button>
        </div>
      </div>
      {banner !== null && banner.tone !== "info" && (
        <div className={`banner banner-${banner.tone}`} role="status">
          <span>{banner.text}</span>
          {banner.action === "clear_halt" && (
            <button type="button" className="btn btn-sm" onClick={() => void run("clear_halt")}>clear halt</button>
          )}
          {banner.action === "open_settings" && !settingsOpen && (
            <button type="button" className="btn btn-sm" onClick={onToggleSettings}>open settings</button>
          )}
        </div>
      )}
    </header>
  );
}
```
`.header` is `display: flex; flex-direction: column; gap: 10px`.

- [ ] **Step 5: Verify** — `npm test`, `npm run build` green. Mock preview (Task 6 Step 6 procedure): header matches the design's top bar (title, "3 accounts", green chip "polling every 60 s", two buttons). The old table still renders under it — that is expected at this commit.

- [ ] **Step 6: Commit**

```bash
git add package.json package-lock.json src/main.tsx src/styles.css src/App.tsx src/components/Header.tsx
git commit -m "feat(ui): dark token sheet, bundled typefaces, app shell and header"
```

---

### Task 8: Accounts table — grid, column drag, row drag, cells

**Files:**
- Create: `src/components/Meter.tsx`, `src/components/AccountRow.tsx`
- Modify: `src/components/AccountsTable.tsx` (rewrite), `src/components/Sparkline.tsx`, `src/App.tsx` (switch the `AccountsTable` call site)

**Interfaces:**
- Consumes: `COLUMNS`, `gridTemplate`, `ColumnKey` (columns.ts); `rowDragTarget`, `rowShift`, `colDragTarget`, `colLineX` (drag.ts); `moveItem` (reorder.ts); `statusPill` (pill.ts); `accountDotColor`, `summarizeModels`, `weekNote`, `sessionNote` (present.ts); `metricColor` (theme.ts); `formatAgo` (format.ts); `buildSparklinePaths` (sparkline.ts); `backend()`.
- Produces:
  - `AccountsTable` props: `{ rows: AccountRow[]; history: Record<string, HistoryPoint[]>; now: number; columnOrder: ColumnKey[]; onColumnOrder: (order: ColumnKey[]) => void; onChanged: () => void; onError: (m: string) => void; onShowFailure: (snapshotId: number) => void }`
  - `AccountRow` props: `{ row: AccountRow; index: number; total: number; points: HistoryPoint[]; now: number; columnOrder: readonly ColumnKey[]; gridCols: string; hotColumn: number | null; drag: RowDragState | null; rowH: number; chartOpen: boolean; editing: boolean; onHandleDown: (e: React.PointerEvent<HTMLDivElement>) => void; onToggleChart: () => void; onToggleEdit: () => void; onMove: (delta: number) => void; onChanged: () => void; onError: (m: string) => void; onShowFailure: (id: number) => void }`
  - `interface RowDragState { index: number; target: number; startY: number; dy: number }` and `interface ColDragState { index: number; target: number; startX: number; dx: number; lineX: number }` (both exported from AccountsTable.tsx)
  - `Meter` props: `{ pct: number | null; note: string; noteWarn?: boolean; title?: string }` — `pct === null` renders an empty track, pct text `—`.
  - `Sparkline` props become `{ points: HistoryPoint[]; stroke: string }` and it renders `<svg viewBox="0 0 100 24" preserveAspectRatio="none">` with `buildSparklinePaths(points, 100, 24)`, `vectorEffect="non-scaling-stroke"`, `strokeWidth={1.6}`; empty → `<span className="spark-empty">—</span>`.

- [ ] **Step 1: Meter and Sparkline**

`src/components/Meter.tsx`:
```tsx
import type { JSX } from "react";
import { metricColor } from "../lib/theme";

interface Props { pct: number | null; note: string; noteWarn?: boolean; title?: string }

export function Meter({ pct, note, noteWarn = false, title }: Props): JSX.Element {
  const clamped = pct === null ? 0 : Math.max(0, Math.min(100, pct));
  return (
    <div className="meter" title={title}>
      <div className="meter-track">
        {pct !== null && (
          <div className="meter-fill" style={{ width: `${clamped}%`, background: metricColor(clamped) }} />
        )}
      </div>
      <div className="meter-meta">
        <span className="meter-pct">{pct === null ? "—" : `${Math.round(clamped)}%`}</span>
        <span className="meter-sep">·</span>
        <span className={`meter-note${noteWarn ? " meter-note-warn" : ""}`}>{note}</span>
      </div>
    </div>
  );
}
```
Sparkline: rewrite per the interface above (keep the `role="img"` and `aria-label`).

- [ ] **Step 2: AccountsTable**

Rewrite `src/components/AccountsTable.tsx`. State: `order: AccountRow[]` (optimistic, resynced from `rows` as today), `drag: RowDragState | null`, `colDrag: ColDragState | null`, `chartId: string | null`, `editingId: string | null`.

Both drags share one pattern so the `window` listeners are always the same function objects and are always removed: the live drag values and the latest `order`/`columnOrder` live in refs; the move/up handlers are created **once** with `useRef` (not `useCallback`, so no dependency drift); a single `useEffect(() => () => detach(), [])` removes whatever is attached at unmount. Exact code:

```tsx
import type { JSX, PointerEvent as ReactPointerEvent } from "react";
import { useEffect, useRef, useState } from "react";
import { backend } from "../lib/backend";
import { COLUMNS, type ColumnKey, gridTemplate } from "../lib/columns";
import { type Rect, colDragTarget, colLineX, rowDragTarget } from "../lib/drag";
import { moveItem } from "../lib/reorder";
import type { AccountRow as AccountRowData, HistoryPoint } from "../lib/types";
import { AccountRow } from "./AccountRow";

export interface RowDragState { index: number; target: number; startY: number; dy: number }
export interface ColDragState { index: number; target: number; startX: number; dx: number; lineX: number }

interface Props {
  rows: AccountRowData[];
  history: Record<string, HistoryPoint[]>;
  now: number;
  columnOrder: ColumnKey[];
  onColumnOrder: (order: ColumnKey[]) => void;
  onChanged: () => void;
  onError: (message: string) => void;
  onShowFailure: (snapshotId: number) => void;
}

const DEFAULT_ROW_H = 66;

function beginBodyDrag(): void {
  document.body.style.cursor = "grabbing";
  document.body.style.userSelect = "none";
}
function endBodyDrag(): void {
  document.body.style.cursor = "";
  document.body.style.userSelect = "";
}

export function AccountsTable({ rows, history, now, columnOrder, onColumnOrder, onChanged, onError, onShowFailure }: Props): JSX.Element {
  const [order, setOrder] = useState<AccountRowData[]>(rows);
  const [drag, setDrag] = useState<RowDragState | null>(null);
  const [colDrag, setColDrag] = useState<ColDragState | null>(null);
  const [chartId, setChartId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);

  // Refs mirror the values the stable window handlers need to read.
  const orderRef = useRef(order);
  const columnOrderRef = useRef(columnOrder);
  const dragRef = useRef<RowDragState | null>(null);
  const colDragRef = useRef<ColDragState | null>(null);
  const rowH = useRef(DEFAULT_ROW_H);
  const colRects = useRef<Rect[]>([]);
  const wrapLeft = useRef(0);
  const onColumnOrderRef = useRef(onColumnOrder);

  useEffect(() => { setOrder(rows); }, [rows]);
  useEffect(() => { orderRef.current = order; }, [order]);
  useEffect(() => { columnOrderRef.current = columnOrder; }, [columnOrder]);
  useEffect(() => { onColumnOrderRef.current = onColumnOrder; }, [onColumnOrder]);

  const commitOrder = async (next: AccountRowData[]): Promise<void> => {
    setOrder(next);
    try {
      await backend().invoke("reorder_accounts", { ids: next.map((r) => r.account.id) });
      onChanged();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
      onChanged(); // optimistic order may not match what was persisted; resync
    }
  };
  const commitOrderRef = useRef(commitOrder);
  useEffect(() => { commitOrderRef.current = commitOrder; });

  /* ---- stable window handlers (created once) ---- */
  const handlers = useRef({
    rowMove: (e: PointerEvent): void => {
      const d = dragRef.current;
      if (d === null) return;
      const dy = e.clientY - d.startY;
      const target = rowDragTarget(d.index, dy, rowH.current, orderRef.current.length);
      dragRef.current = { ...d, dy, target };
      setDrag(dragRef.current);
    },
    rowUp: (): void => {
      const d = dragRef.current;
      detachRow();
      dragRef.current = null;
      setDrag(null);
      if (d !== null && d.target !== d.index) {
        void commitOrderRef.current(moveItem(orderRef.current, d.index, d.target));
      }
    },
    colMove: (e: PointerEvent): void => {
      const c = colDragRef.current;
      if (c === null) return;
      const target = colDragTarget(colRects.current, e.clientX);
      colDragRef.current = { ...c, dx: e.clientX - c.startX, target, lineX: colLineX(colRects.current, target, c.index, wrapLeft.current) };
      setColDrag(colDragRef.current);
    },
    colUp: (): void => {
      const c = colDragRef.current;
      detachCol();
      colDragRef.current = null;
      setColDrag(null);
      if (c !== null && c.target !== c.index) {
        onColumnOrderRef.current(moveItem(columnOrderRef.current, c.index, c.target));
      }
    },
  });

  function detachRow(): void {
    window.removeEventListener("pointermove", handlers.current.rowMove);
    window.removeEventListener("pointerup", handlers.current.rowUp);
    endBodyDrag();
  }
  function detachCol(): void {
    window.removeEventListener("pointermove", handlers.current.colMove);
    window.removeEventListener("pointerup", handlers.current.colUp);
    endBodyDrag();
  }
  useEffect(() => () => { detachRow(); detachCol(); }, []);

  const startRowDrag = (e: ReactPointerEvent<HTMLDivElement>, index: number): void => {
    e.preventDefault();
    const grid = e.currentTarget.parentElement;
    rowH.current = grid !== null ? grid.offsetHeight + 1 : DEFAULT_ROW_H;
    beginBodyDrag();
    setChartId(null);
    setEditingId(null);
    dragRef.current = { index, target: index, startY: e.clientY, dy: 0 };
    setDrag(dragRef.current);
    window.addEventListener("pointermove", handlers.current.rowMove);
    window.addEventListener("pointerup", handlers.current.rowUp);
  };

  const startColDrag = (e: ReactPointerEvent<HTMLDivElement>, index: number): void => {
    e.preventDefault();
    const head = e.currentTarget.parentElement;
    if (head === null) return;
    colRects.current = Array.from(head.querySelectorAll<HTMLElement>("[data-colcell]")).map((el) => {
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, width: r.width };
    });
    wrapLeft.current = (head.parentElement ?? head).getBoundingClientRect().left;
    beginBodyDrag();
    setChartId(null);
    setEditingId(null);
    colDragRef.current = { index, target: index, startX: e.clientX, dx: 0, lineX: colLineX(colRects.current, index, index, wrapLeft.current) };
    setColDrag(colDragRef.current);
    window.addEventListener("pointermove", handlers.current.colMove);
    window.addEventListener("pointerup", handlers.current.colUp);
  };

  const moveBy = (index: number, delta: number): void => {
    const target = index + delta;
    if (target < 0 || target >= order.length) return;
    void commitOrder(moveItem(order, index, target));
  };
  // …render below
}
```
`detachRow`/`detachCol` are function declarations (hoisted) so the `handlers` initialiser can reference them. `useEffect(() => { commitOrderRef.current = commitOrder; })` runs every render on purpose so the ref always holds the closure over the current `onChanged`/`onError`.

Render:
```tsx
const gridCols = gridTemplate(columnOrder);
return (
  <section className="panel" aria-label="Accounts">
    <div className="thead-wrap">
      <div className="thead" style={{ gridTemplateColumns: gridCols }}>
        <div />
        {columnOrder.map((key, ci) => {
          const dragging = colDrag !== null && colDrag.index === ci;
          const cls = ["th", dragging ? "th-dragging" : "", colDrag !== null && !dragging ? "th-dimmed" : ""].filter(Boolean).join(" ");
          return (
            <div key={key} data-colcell="1" className={cls} title="Drag to move column"
              style={{ transform: dragging ? `translateX(${colDrag.dx}px)` : undefined }}
              onPointerDown={(e) => startColDrag(e, ci)}>
              {COLUMNS[key].label}
            </div>
          );
        })}
        <div />
      </div>
      {colDrag !== null && <div className="col-line" style={{ left: `${colDrag.lineX}px` }} />}
    </div>
    <div className="rows">
      {drag !== null && (
        <div className="row-placeholder" style={{ top: `${drag.target * rowH.current + 6}px`, height: `${rowH.current - 12}px` }} />
      )}
      {order.map((row, idx) => (
        <AccountRow key={row.account.id} row={row} index={idx} total={order.length}
          points={history[row.account.id] ?? []} now={now} columnOrder={columnOrder} gridCols={gridCols}
          hotColumn={colDrag?.index ?? null} drag={drag} rowH={rowH.current}
          chartOpen={chartId === row.account.id} editing={editingId === row.account.id}
          onHandleDown={(e) => startRowDrag(e, idx)}
          onToggleChart={() => { setEditingId(null); setChartId((c) => (c === row.account.id ? null : row.account.id)); }}
          onToggleEdit={() => { setChartId(null); setEditingId((c) => (c === row.account.id ? null : row.account.id)); }}
          onMove={(delta) => moveBy(idx, delta)}
          onChanged={onChanged} onError={onError} onShowFailure={onShowFailure} />
      ))}
    </div>
  </section>
);
```
`moveBy(idx, delta)` is today's `moveByKeyboard`.

- [ ] **Step 3: AccountRow (cells and the edit button; drawers arrive in Tasks 9–10)**

`src/components/AccountRow.tsx` renders:
```tsx
const pill = statusPill(row, now);
const session = row.latest?.session ?? null;
const week = row.latest?.week_all ?? null;
const models = summarizeModels(row.latest?.week_models ?? []);
const weekPct = week?.pct ?? 0;
const stroke = metricColor(weekPct);

let style: CSSProperties = {};
let lifted = false, parked = false;
if (drag !== null) {
  if (drag.index === index) { lifted = true; style = { transform: `translateY(${drag.dy}px) scale(1.012)` }; }
  else { parked = true; style = { transform: `translateY(${rowShift(index, drag.index, drag.target, rowH)}px)` }; }
}

const cell = (key: ColumnKey): JSX.Element => {
  switch (key) {
    case "account": return (
      <div className="acct">
        <span className="acct-dot" style={{ background: accountDotColor(row, pill) }} />
        <span className={`acct-name${row.account.enabled ? "" : " acct-name-off"}`} title={row.account.config_dir}>{row.account.label}</span>
        {row.account.is_default && <span className="tag">default</span>}
        {pill.tone !== "success" && <StatusPill pill={pill} onShowFailure={onShowFailure} />}
      </div>);
    case "session": return <Meter pct={session?.pct ?? null} note={sessionNote(session, now)} />;
    case "week": { const n = weekNote(weekPct); return <Meter pct={week?.pct ?? null} note={week === null ? "no data" : n.text} noteWarn={n.warn} />; }
    case "model": return models === null ? <Meter pct={null} note="no data" /> : <Meter pct={models.pct} note={models.note} title={models.title} />;
    case "spark": return (
      <button type="button" className={`spark${chartOpen ? " spark-open" : ""}`} title="Click for 30 days" onClick={onToggleChart}>
        <Sparkline points={last7(points, now)} stroke={stroke} />
      </button>);
    case "updated": return <span className="updated">{formatAgo(row.latest?.taken_at ?? null, now)}</span>;
  }
};
```
`StatusPill` is a small component in the same file; a failed poll is a button that opens the detail, everything else is inert text:
```tsx
function StatusPill({ pill, onShowFailure }: { pill: Pill; onShowFailure: (id: number) => void }): JSX.Element {
  const cls = `pill pill-${pill.kind} pill-tone-${pill.tone}`;
  const id = pill.snapshotId;
  if (id !== undefined && pill.outcome !== "ok") {
    return (
      <button type="button" className={cls} title={pill.tooltip} onClick={() => onShowFailure(id)}>
        {pill.label}
      </button>
    );
  }
  return <span className={cls} title={pill.tooltip}>{pill.label}</span>;
}
```
`last7(points, now)` filters `t >= now - 7 * 86_400_000` (a local helper in AccountRow.tsx; trivial enough not to need its own test, but if it grows, move it to series.ts with a test).

The row:
```tsx
<div className={["row", lifted ? "row-lifted" : "", parked ? "row-parked" : ""].filter(Boolean).join(" ")} style={style}>
  <div className="row-grid" style={{ gridTemplateColumns: gridCols }}>
    <div className="grip" role="button" aria-label="Drag to reorder" title="Drag to reorder rows" onPointerDown={onHandleDown}>
      {/* six 3px dots: three rows of two */}
    </div>
    {columnOrder.map((key, ci) => (
      <div key={key} className={`cell${hotColumn === ci ? " cell-hot" : ""}`}>{cell(key)}</div>
    ))}
    <div className="row-actions">
      <button type="button" className={`btn btn-sm${editing ? " btn-edit-on" : ""}`} onClick={onToggleEdit}>edit</button>
    </div>
  </div>
  {chartOpen && <HistoryDrawer …/>}   {/* Task 9 */}
  {editing && <EditDrawer …/>}        {/* Task 10 */}
</div>
```

- [ ] **Step 4: Switch `App.tsx`** — change `const { prefs } = usePrefs();` to `const { prefs, update } = usePrefs();` and replace the `AccountsTable` call with:
```tsx
<AccountsTable
  rows={dashboard.accounts}
  history={history}
  now={now}
  columnOrder={prefs.columnOrder}
  onColumnOrder={(columnOrder) => update({ columnOrder })}
  onChanged={refetch}
  onError={showError}
  onShowFailure={(id) => setFailureId(id)}
/>
```

- [ ] **Step 5: Verify** — `npm test`, `npm run build`, `npx tsc --noEmit`. Mock preview: table matches the design's header strip and rows (dots, meters with `pct · note`, sparkline, updated). Use the Playwright MCP to drag a column header (`browser_drag` from "Updated" onto "Session") and confirm the order changes and survives a reload (localStorage). Drag a row handle: `browser_evaluate` dispatching `pointerdown`/`pointermove`/`pointerup` on `.grip` with `clientY` offsets, then confirm the order changed. Enable the FailureDetail path: the `claude2` mock row shows a `timeout` pill; clicking it opens the modal.

- [ ] **Step 6: Commit**

```bash
git add src/components/Meter.tsx src/components/AccountRow.tsx src/components/AccountsTable.tsx src/components/Sparkline.tsx src/App.tsx
git commit -m "feat(ui): grid accounts table with pointer row/column reordering"
```

---

### Task 9: 30-day history drawer

**Files:**
- Create: `src/components/HistoryDrawer.tsx`
- Modify: `src/components/AccountRow.tsx` (mount it)

**Interfaces:**
- Consumes: `dailyMax`, `polylinePoints`, `seriesDots`, `seriesStats`, `dayLabel`, `axisLabels` (series.ts); `HISTORY_DAYS` (useDashboard.ts).
- Produces: `HistoryDrawer` props `{ points: HistoryPoint[]; now: number; stroke: string; onCollapse: () => void }`.

- [ ] **Step 1: Implement**

```tsx
import type { JSX } from "react";
import { useState } from "react";
import { HISTORY_DAYS } from "../hooks/useDashboard";
import { axisLabels, dailyMax, dayLabel, polylinePoints, seriesDots, seriesStats } from "../lib/series";
import type { HistoryPoint } from "../lib/types";

interface Props { points: HistoryPoint[]; now: number; stroke: string; onCollapse: () => void }
interface Tip { left: string; bottom: string; edge: "left" | "mid" | "right"; text: string }

const TIP_TRANSFORM: Record<Tip["edge"], string> = {
  right: "translate(-100%, calc(100% + 14px))",
  left: "translate(0, calc(100% + 14px))",
  mid: "translate(-50%, calc(100% + 14px))",
};

export function HistoryDrawer({ points, now, stroke, onCollapse }: Props): JSX.Element {
  const [tip, setTip] = useState<Tip | null>(null);
  const days = dailyMax(points, HISTORY_DAYS, now);
  const stats = seriesStats(days);
  const line = polylinePoints(days, 100, 100);
  const dots = seriesDots(days);
  return (
    <div className="drawer">
      <div className="drawer-head">
        <span className="drawer-label">Last 30 days · weekly limit use</span>
        <div className="drawer-stats">
          <span>peak {stats.peak}%</span>
          <span>avg {stats.avg}%</span>
          <span>missing {stats.missing} d</span>
          <button type="button" className="btn-link" onClick={onCollapse}>collapse</button>
        </div>
      </div>
      <div className="chart">
        <svg viewBox="0 0 100 100" preserveAspectRatio="none" role="img" aria-label="weekly usage, last 30 days">
          {[0, 50, 100].map((y) => <line key={y} x1={0} y1={y} x2={100} y2={y} stroke="#1e252a" strokeWidth={1} vectorEffect="non-scaling-stroke" />)}
          {line !== "" && <polyline points={line} fill="none" stroke={stroke} strokeWidth={1.8} vectorEffect="non-scaling-stroke" strokeLinejoin="round" strokeLinecap="round" />}
        </svg>
        <div className="chart-dots">
          {dots.map((d) => {
            const left = `${d.leftPct.toFixed(2)}%`;
            const bottom = `${d.value}%`;
            const edge = d.leftPct > 78 ? "right" : d.leftPct < 22 ? "left" : "mid";
            const text = `${dayLabel(d.index, HISTORY_DAYS, now)} · ${d.value}%`;
            return (
              <div key={d.index} className="dot" style={{ left, bottom }}
                onMouseEnter={() => setTip({ left, bottom, edge, text })}
                onMouseLeave={() => setTip(null)} />
            );
          })}
          {tip !== null && (
            <div className="tip" style={{ left: tip.left, bottom: tip.bottom, transform: TIP_TRANSFORM[tip.edge] }}>{tip.text}</div>
          )}
        </div>
        <span className="chart-y chart-y-top">100%</span>
        <span className="chart-y chart-y-bottom">0%</span>
      </div>
      <div className="chart-axis">
        {axisLabels(HISTORY_DAYS, 4, now).map((t) => <span key={t}>{t}</span>)}
      </div>
    </div>
  );
}
```
Mount in `AccountRow`: `{chartOpen && <HistoryDrawer points={points} now={now} stroke={stroke} onCollapse={onToggleChart} />}`.

- [ ] **Step 2: Verify** — gates green; mock preview: click a sparkline → drawer with 30 dots (minus the two `null` days for `claude3`: "missing 2 d"), hover a dot → tooltip "Sep 5 · 55%"-style text. Screenshot and compare with the design's drawer.

- [ ] **Step 3: Commit**

```bash
git add src/components/HistoryDrawer.tsx src/components/AccountRow.tsx
git commit -m "feat(ui): 30-day history drawer with per-day markers and tooltip"
```

---

### Task 10: Edit drawer

**Files:**
- Create: `src/components/EditDrawer.tsx`
- Modify: `src/components/AccountRow.tsx` (mount it)

**Interfaces:**
- Consumes: `backend()`, `formatCountdown` (format.ts).
- Produces: `EditDrawer` props `{ row: AccountRow; index: number; total: number; now: number; onClose: () => void; onMove: (delta: number) => void; onChanged: () => void; onError: (m: string) => void }`.

- [ ] **Step 1: Implement**

Rows (each `.edit-row` with an `.edit-label`):
1. **Name**: `<input className="input" value={draft} autoFocus onChange onKeyDown(Enter → save, Escape → onClose)>`, `save` (`.btn .btn-sm .btn-primary`) → `update_account { id, label: draft.trim() || row.account.label }` then `onClose()`; `cancel` (`.btn .btn-sm .btn-ghost`) → `onClose()`.
2. **Resets** (read-only, mono hints): `session · ${session === null ? "no data" : formatCountdown(session.resets_at, now)}` and `weekly window · ${week === null ? "no data" : formatCountdown(week.resets_at, now)}`; when `week_models.length > 0`, one more hint per model: `${label} · ${formatCountdown(resets_at, now)}`.
3. **Path**: `<span className="edit-path" title={config_dir}>{config_dir}</span>`.
4. **Account**: `enable`/`disable` (`update_account { id, enabled: !enabled }`), `log in` (`open_login { id }`), `move up` (disabled at index 0) / `move down` (disabled at `total - 1`) → `onMove(∓1)`, and `remove` (`.btn .btn-sm .btn-danger .ml-auto`) → `remove_account { id }` then `onClose()`.

All backend calls go through one local `call(command, args)` with try/catch → `onError`, then `onChanged()`.

Mount in `AccountRow`: `{editing && <EditDrawer row={row} index={index} total={total} now={now} onClose={onToggleEdit} onMove={onMove} onChanged={onChanged} onError={onError} />}`.

- [ ] **Step 2: Verify** — gates green. Mock preview: edit → rename → save updates the label; disable dims the name and shows the `disabled` pill; move down reorders; remove drops the row. Escape closes. No `!` in the file.

- [ ] **Step 3: Commit**

```bash
git add src/components/EditDrawer.tsx src/components/AccountRow.tsx
git commit -m "feat(ui): inline edit drawer with rename, reset times, enable, login, move and remove"
```

---

### Task 11: Settings panel, toggle, failure modal and toast restyle

**Files:**
- Create: `src/components/Toggle.tsx`
- Modify: `src/components/Settings.tsx` (rewrite), `src/components/FailureDetail.tsx` (class names only), `src/App.tsx` (switch the `Settings` call site)

**Interfaces:**
- Consumes: `FONTS`, `FONT_KEYS`, `SIZES`, `SIZE_KEYS` (theme.ts); `Prefs` (prefs.ts); `backend()`; `UserSettings`, `BinaryInfo` (types.ts).
- Produces: `Settings` props `{ binary: BinaryInfo; prefs: Prefs; onPrefs: (patch: Partial<Prefs>) => void; onClose: () => void; onChanged: () => void; onError: (m: string) => void }`; `Toggle` props `{ label: string; hint: string; checked: boolean; onChange: (next: boolean) => void }` rendering `<button type="button" role="switch" aria-checked={checked} className="toggle-row">…<span className={"switch" + (checked ? " switch-on" : "")}><span className="knob"/></span></button>`.

- [ ] **Step 1: Settings layout** (top to bottom, per the design):
1. `.settings-head`: title "Settings", `close` (`.btn .btn-sm .btn-ghost`).
2. **Typeface** `.section`: one `.choice` button per `FONT_KEYS` entry — `.choice-sample` shows `FONTS[k].label` in `FONTS[k].ui`, `.choice-sub` shows `46% · 1 min ago` in `FONTS[k].mono`; `.choice-on` when `prefs.font === k`; click → `onPrefs({ font: k })`.
3. **Text size** `.section`: `.choice .choice-size` per `SIZE_KEYS` with `SIZES[k].label`; click → `onPrefs({ size: k })`.
4. `.divider`.
5. `.settings-grid` with two `.field`s: **Poll gap** (`.input .input-mono .input-num`, `type="number" min=10 max=3600`, hint `seconds · 10–3600`) and **Poll timeout** (`min=5 max=120`, hint `seconds · 5–120`). Both are *drafts*: local `useState<string>` seeded from settings; commit on blur or Enter via `save({ ...settings, interval_secs: Number(draft) })`; on backend error restore the previous value into both `settings` and the draft (today's `save` already restores `settings`).
6. **Binary override** `.field`: text `.input .input-mono` (max-width 520px), placeholder `leave blank to auto-detect`, draft → save on blur/Enter; hint `detected: {binary.path ?? "none"}{source ? ` (${source})` : ""}`.
7. `.divider`.
8. `.toggles`: `Close to tray` / `keep polling in the background`; `Launch at login` / `start with the system`; `Debug logging` / `verbose logs, pruned after 30 days` (design copy verbatim; it doubles as the spec's retention note). Each `Toggle` → `save({...})` (debug maps to `log_level: "debug" | "info"`).
9. `.accounts-row`: `.input .input-mono .input-grow` (placeholder `path to a config directory`) + `add account` (`.btn`, submits `add_account { configDir }` and clears the field) + `rescan profiles` (`.btn`) + `open logs` (`.btn .btn-ghost`, `open_log_dir`). Wrap input + add in a `<form>` so Enter submits.

Keep `Loading…` as `<section className="panel settings"><p className="loading">loading…</p></section>` while settings load.

- [ ] **Step 2: FailureDetail** — keep logic; classes: `.modal` / `.modal-body`, title `.settings-title`, labels `.section-label`, close button `.btn .btn-sm .btn-ghost`. Add `onKeyDown` Escape → `onClose` on the dialog and `autoFocus` the close button.

- [ ] **Step 3: App.tsx** — replace the `Settings` call with:
```tsx
<Settings
  binary={dashboard.binary}
  prefs={prefs}
  onPrefs={update}
  onClose={() => setShowSettings(false)}
  onChanged={refetch}
  onError={showError}
/>
```

- [ ] **Step 4: Verify** — gates green. Mock preview: settings panel matches the design; choosing IBM Plex changes the whole page's typeface; choosing "largest" zooms; typing `5` into Poll gap and blurring shows the `out_of_range` toast and restores `60`; toggles animate; `add account` with `C:\Users\josh\.claude4` adds a disabled row. Reload: font/size persist.

- [ ] **Step 5: Commit**

```bash
git add src/components/Toggle.tsx src/components/Settings.tsx src/components/FailureDetail.tsx src/App.tsx
git commit -m "feat(ui): settings card with typeface/text size pickers, switches and account tools"
```

---

### Task 12: Error messages, docs, design assets, visual verification and final gates

**Files:**
- Create: `src/lib/errors.ts`, `src/lib/errors.test.ts`
- Modify: `src/components/Header.tsx`, `src/components/AccountsTable.tsx`, `src/components/EditDrawer.tsx`, `src/components/Settings.tsx`, `src/components/FailureDetail.tsx`, `src/hooks/useDashboard.ts`, `src/lib/mockBackend.ts`, `README.md`
- Add: `docs/design/usage-tracker.dc.html`, `docs/design/usage-tracker-kit.js` (already on disk, untracked), this plan.

**Interfaces:**
- Produces: `errorMessage(e: unknown): string` — `isAppError(e)` → `e.message`; `e instanceof Error` → `e.message`; otherwise `String(e)`.

- [ ] **Step 0a: Error message helper (TDD).** Tauri rejects a command with the serialized `AppError` `{ code, message }` (see `src-tauri/src/error.rs`), so the pattern `e instanceof Error ? e.message : String(e)` used at every call site renders toasts as `[object Object]`. Write `src/lib/errors.test.ts` first:
```ts
import { describe, expect, it } from "vitest";
import { errorMessage } from "./errors";

describe("errorMessage", () => {
  it("unwraps a serialized AppError", () => {
    expect(errorMessage({ code: "out_of_range", message: "interval_secs must be 10..=3600, got 5" }))
      .toBe("interval_secs must be 10..=3600, got 5");
  });
  it("unwraps an Error", () => {
    expect(errorMessage(new Error("boom"))).toBe("boom");
  });
  it("stringifies anything else", () => {
    expect(errorMessage("plain")).toBe("plain");
    expect(errorMessage(42)).toBe("42");
    expect(errorMessage({ nope: true })).toBe("[object Object]");
  });
});
```
Run it (FAIL: module not found), then create `src/lib/errors.ts`:
```ts
import { isAppError } from "./types";

/** Tauri rejects with the serialized AppError object; surface its message. */
export function errorMessage(e: unknown): string {
  if (isAppError(e)) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}
```
Run again (PASS). Then replace every `e instanceof Error ? e.message : String(e)` in `src/` with `errorMessage(e)` (grep to find them all; import the helper in each file) and confirm `grep -rn "instanceof Error" src` returns nothing outside `errors.ts`.

- [ ] **Step 0b: Mock history realism.** In `src/lib/mockBackend.ts`, emit hourly points from 09:00 to 18:00 local for each non-null day (pct rising from `v-8` to `v` then easing to `v-3`, clamped ≥ 0) instead of three points three hours apart, so the 7-day sparkline renders as connected runs under the mock the way real hourly polling does. Keep the same day values so the 30-day drawer's stats are unchanged.

- [ ] **Step 1: README** — replace the "Accounts can be reordered…" paragraph with: row drag handle **or** Move up/Move down inside a row's edit drawer; column headers drag to reorder and the order, typeface and text size persist per machine in the webview's local storage; click a sparkline for the 30-day drawer. Add a "Browser preview" subsection:
```bash
# renders the UI with an in-memory mock backend, no Rust build needed
VITE_MOCK_BACKEND=1 npm run dev        # PowerShell: $env:VITE_MOCK_BACKEND='1'; npm run dev
```
State that the mock is never bundled into a production build (dynamic import behind the env flag).

- [ ] **Step 2: Visual verification** — with the mock server up, take Playwright screenshots at 1160×760 of: the dashboard, an open chart drawer, an open edit drawer, the settings panel with IBM Plex + large selected. Save them under the scratchpad directory (not the repo) and compare each against the design side by side. Fix pixel-level deviations that are obvious (spacing, sizes, colours); do not change behaviour here.

- [ ] **Step 3: Real app boot** — `npm run tauri dev` once, confirm the window renders the real accounts and the tray still works, then close it. This is the only step that touches the real CLI and it spends nothing (no poll is triggered by opening the window beyond the scheduler's own behaviour).

- [ ] **Step 4: All four gates** — run and paste the tail of each into the commit message body or the PR description.

- [ ] **Step 5: Commit**

Two commits: first the code (`git add src/lib/errors.ts src/lib/errors.test.ts src/components/*.tsx src/hooks/useDashboard.ts src/lib/mockBackend.ts` → `fix(ui): surface backend error messages in toasts; hourly mock history`), then the docs:
```bash
git add README.md docs/design docs/superpowers/plans/2026-09-16-ui-overhaul.md
git commit -m "docs: UI overhaul reference design, README for the new dashboard"
```

---

## Self-review

- **Coverage vs the design template:** top bar (T7), chip (T5/T7), table header + column drag (T8), rows + row drag + placeholder (T8), account/meter/spark/updated cells (T8), edit button (T8), chart drawer with dots/tooltip/axis/stats (T9), edit drawer (T10, with the deviations table), hint line (T7), settings: typeface, text size, poll gap/timeout, binary override, toggles, add/rescan/logs (T11). Spec §7 items retained: banner states + actions (T7), pill precedence and failure detail (T8), Move up/down (T10), retention note (T11 toggle hint).
- **Types:** `RowDragState` defined in T8 and used only there; `Backend.listen` returns `() => void` and `useDashboard` calls `off()` — consistent; `Chip.dot` values match `.chip-dot-*` classes in T7; `Prefs.columnOrder` is `ColumnKey[]` and `AccountsTable.columnOrder` is `ColumnKey[]`; `HISTORY_DAYS` exported from `useDashboard.ts` and imported in `HistoryDrawer.tsx` (a hook module exporting a constant is fine; it does not create a cycle).
- **Placeholders:** none.
