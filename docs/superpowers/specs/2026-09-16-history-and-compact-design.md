# History range, hideable columns, compact view — design

Date: 2026-09-16. Branch: `history-and-compact`. Builds on the UI overhaul
(`docs/superpowers/plans/2026-09-16-ui-overhaul.md`) and the behavioural
contract in `docs/2026-09-15-claude-usage-tracker-design.md` §7–§8.

## 1. Goal

Four changes to the Claude Usage Tracker, shipped together on one branch:

1. **Chart range, granularity and metric.** The per-account history drawer
   gains range presets (1h … 30d), a granularity the user can override, and
   a metric picker (week-all, session, or any model label). The backend
   query does the bucketing for every combination inside hard limits.
2. **Hideable columns.** A "Columns" section in Settings hides any column
   except Account; grid template, header strip, cells and column drag all
   operate on the visible columns only.
3. **Compact view.** The minimum window shrinks to 360×240. Below 820 px the
   table auto-hides two columns; below 640 px it becomes one card per
   account with ring gauges. A "Keep window on top" toggle makes the small
   window useful.
4. **Remove the default-account flag** end to end (UI tag, DTO field,
   discovery step, database column). Manual `sort_order` is already the
   only ordering that matters.

Decisions confirmed with Josh on 2026-09-16 (do not re-open inside tasks):
metric set = week-all + session + every model label seen in the account's
history; presets/limits per §3.2; compact thresholds and minimum window
360×240 (thresholds corrected in §5.2 after review: the 660 / 500 figures
Josh accepted came from wrong column arithmetic, see there); ring gauges, not
pies; always-on-top as a Settings toggle persisted in prefs; default flag
removed end to end; **no column is hidden by default** (Josh withdrew the
"hide Session first" request); the hint line under the table ("drag a row
handle to set failover priority · …") is removed outright (Josh,
2026-09-16).

## 2. Facts this design relies on (verified 2026-09-16)

- Every poll writes one `snapshots` row: `taken_at`, `outcome`,
  `session_pct`, `week_all_pct`, `week_models` (JSON array of
  `{label, pct, resets_at}`), kept 30 days (`RETENTION_MS`). Density is one
  row per 1.7–3.4 min while Claude Code runs and nothing while idle, so
  empty buckets are **breaks, never zeros**.
- Bundled SQLite is 3.53.2 (`libsqlite3-sys 0.38.2`): `json_each` and
  `ALTER TABLE … DROP COLUMN` are both available. Index
  `snapshots_acct_time(account_id, taken_at DESC, id DESC)` serves every
  range scan below.
- `Store::history` is the only place that buckets; it takes `since` and a
  hard-coded `HOUR_MS`. `get_history` / `core_get_history` wrap it with a
  `days` argument (1–30, default 7). `useDashboard` asks for 30 days once per
  cycle; `AccountRow` filters to 7 days for the sparkline; `HistoryDrawer`
  derives local-day maxima with `dailyMax`.
- The chart is hand-rolled SVG: `polylineRuns` emits one `<polyline>` per
  contiguous run of known values; `seriesDots` places hover targets;
  `seriesStats` computes peak/avg/missing. No chart library; that stays.
- View prefs live in `localStorage` under `usage-tracker.prefs.v1`
  (`parsePrefs` validates each field independently; `normalizeColumnOrder`
  migrates the order). `usePrefs` exposes `{prefs, update}`.
- Text size is CSS `zoom` on `main.app`. Anything measured in viewport px
  must go through `toLocal(px, zoom)` (`src/lib/drag.ts`).
- `VITE_MOCK_BACKEND=1 npm run dev` serves the UI against
  `src/lib/mockBackend.ts`; visual checks run with the Playwright MCP against
  it. A running release build holds a single-instance lock: stop it before
  `npm run tauri dev`, relaunch afterwards.
- Vitest runs in the node environment on `src/**/*.test.ts` only. Every rule
  that needs a test is a pure function in `src/lib/`.
- `is_default` is written by `seed_accounts_if_empty` (resolved against
  `paths::default_config_dir`), read by the row's `default` tag, and used as
  a tiebreak after `sort_order` in `ORDER BY`. Nothing else depends on it.

## 3. Feature 1 — chart range, granularity, metric

### 3.1 Backend command

`get_history` changes shape (no sibling; the old `days` form has one caller
and it moves too):

```
get_history { account_id: String, since: i64, bucket_ms: i64, metric: HistoryMetric }
  -> Vec<HistoryPoint { t: i64, pct: u8 }>

HistoryMetric (serde, tag = "kind", rename_all = "snake_case"):
  { kind: "week_all" } | { kind: "session" } | { kind: "model", label: String }
```

Validation in `core_get_history(core, account_id, now, since, bucket_ms,
metric)`, every failure an `AppError::OutOfRange` with a message naming the
argument and the bound:

| Rule | Bound |
|---|---|
| `bucket_ms >= MIN_BUCKET_MS` | 60 000 |
| `since <= now` | — |
| `now - since <= MAX_RANGE_MS + RANGE_SLACK_MS` | 30 d + 5 min |
| `ceil((now - since) / bucket_ms) <= MAX_BUCKETS` | 1 000 |
| `metric.label` (model) non-empty after trim, ≤ `MAX_LABEL_LEN` (64) chars | — |

Why reject rather than clamp an over-long range: the client aligns `since`
to a unit boundary and buckets the response against that same `since`
(§3.3). A server that silently moved `since` would return buckets the client
cannot place. The slack only has to cover the gap between the client reading
its clock and the command running (a poll cycle holding the blocking pool,
or a brief suspend); five minutes covers that, and a request older than that
is stale by definition — the drawer refetches on the next cycle anyway.

Bucketing is **anchored at `since`**, not at the epoch:
`bucket = since + ((taken_at - since) / bucket_ms) * bucket_ms`. The client
aligns `since` to a local-time unit boundary (§3.3), so 1-day buckets start
at local midnight and 1-hour buckets on the hour. The value per bucket is
`MAX(metric)` over `outcome = 'ok'` rows, as today; `pct` stays clamped to
0..=100 (`u8`) for wire compatibility.

Three SQL shapes, selected by metric in `Store::history(account_id, since,
bucket_ms, metric)`:

```sql
-- week_all (session is identical with session_pct)
SELECT ?2 + ((taken_at - ?2) / ?3) * ?3 AS bucket, MAX(week_all_pct) AS pct
FROM snapshots
WHERE account_id = ?1 AND taken_at >= ?2 AND outcome = 'ok' AND week_all_pct IS NOT NULL
GROUP BY bucket ORDER BY bucket ASC;

-- model
SELECT ?2 + ((s.taken_at - ?2) / ?3) * ?3 AS bucket,
       MAX(json_extract(s.week_models, m.fullkey || '.pct')) AS pct
FROM snapshots AS s,
     json_each(CASE WHEN json_valid(s.week_models) AND json_type(s.week_models) = 'array'
                    THEN s.week_models ELSE '[]' END) AS m
WHERE s.account_id = ?1 AND s.taken_at >= ?2 AND s.outcome = 'ok'
  AND s.week_models IS NOT NULL
  AND json_extract(s.week_models, m.fullkey || '.label') = ?4
  AND json_type(s.week_models, m.fullkey || '.pct') IN ('integer', 'real')
GROUP BY bucket ORDER BY bucket ASC;
```

This mirrors `row_to_dto`'s tolerance of a malformed `week_models` (it falls
back to an empty list rather than failing). The guard has to sit **inside
`json_each`'s argument**: the function raises on malformed input before any
`WHERE` term can filter the row, so a `WHERE json_valid(...)` would not save
the query. Element fields are read from the root document via `m.fullkey`
rather than from `m.value`, so a scalar element (`[5, {...}]`) yields NULL
instead of a "malformed JSON" error, and the `json_type … IN ('integer',
'real')` predicate keeps a text `pct` out of `MAX`. The model `pct` is read
as `f64` (JSON numbers may come back as REAL), rounded, then clamped to
`u8` like the other two.

**Why server-side `json_each` rather than client aggregation:** the
alternative is shipping every raw row in range (up to ~25 000 per account
per 30 days) to the webview and bucketing in JS. That defeats the
1 000-bucket cap, makes the drawer's cost scale with retention rather than
with the chart, and duplicates the bucketing rule in two languages. The
JSON path keeps one bucketing implementation, one index, and one limit.
`json_each` over a few thousand rows is well under a millisecond per row;
the drawer fetches on demand, not per poll.

A second, read-only command supplies the picker:

```
get_history_models { account_id: String } -> Vec<String>
```

`SELECT DISTINCT json_extract(s.week_models, m.fullkey || '.label') AS label
FROM snapshots s, json_each(CASE WHEN json_valid(s.week_models) AND
json_type(s.week_models) = 'array' THEN s.week_models ELSE '[]' END) m
WHERE s.account_id = ?1 AND s.taken_at >= ?2 AND s.outcome = 'ok' AND
s.week_models IS NOT NULL AND typeof(label) = 'text'
AND length(trim(label, char(32, 9, 10, 13))) >= 1 AND length(label) <= ?3
ORDER BY label ASC`, with `since = now - RETENTION_MS` and `?3 =
MAX_LABEL_LEN` (same malformed-JSON guard as the model query above).
SQLite's bare `trim()` strips only spaces, so the explicit character set
(space, tab, LF, CR) is what makes this the same rule as the validation
table — non-blank after trim, at most 64 characters — and the picker can
never offer a label the metric validation would reject. (Rust's `trim()`
strips more Unicode whitespace than those four; a label made only of, say,
U+00A0 would slip through the query and be rejected — accepted, the CLI
does not emit such labels.)

Rust changes: `HistoryMetric` enum in `store/snapshots.rs` (serialise +
deserialise; `Deserialize` is what the command boundary needs);
`Store::history` and new `Store::history_models`; `core_get_history`
validation; `get_history` / `get_history_models` commands; register in
`lib.rs`. `HOUR_MS` in `store/snapshots.rs`, and `DEFAULT_HISTORY_DAYS`,
`MAX_HISTORY_DAYS`, `DAY_MS` in `commands.rs`, are removed (nothing else
uses them). Constants `MIN_BUCKET_MS`, `MAX_BUCKETS`, `MAX_RANGE_MS`,
`RANGE_SLACK_MS`, `MAX_LABEL_LEN` live next to `RETENTION_MS` in
`store/snapshots.rs`. The three limits the UI also needs are published in
one shared file, `src/lib/historyLimits.json`
(`{"minBucketMs": 60000, "maxBuckets": 1000, "maxRangeMs": 2592000000}`):
`history.ts` imports it (`resolveJsonModule` is on) and derives its
constants from it, so the TypeScript side cannot drift by construction; a
Rust unit test in `snapshots.rs` reads the same file with `include_str!` and
asserts each value equals the Rust constant (and that `MAX_RANGE_MS ==
RETENTION_MS`), so the Rust side cannot drift without failing `cargo test`.
(`node:fs` from a Vitest test was rejected: `@types/node` is not installed
and test files are type-checked by the `tsc` build gate.) `RANGE_SLACK_MS`
and `MAX_LABEL_LEN` have no TypeScript counterpart (the UI never computes
them) and are not in the JSON.

Label validation in Rust: `label.trim()` must be non-empty and
`trim().chars().count() <= MAX_LABEL_LEN` (characters, matching SQLite's
`length()`, not bytes). The history query then matches the label **exactly as
sent**, untrimmed; the trim is only for the emptiness/length check, so a
label the picker offered always round-trips.

Logging: `core_get_history` emits a `debug!` with `account_id`, `since`,
`bucket_ms`, `metric`, and the returned point count; a rejected request
logs at `warn!` with the failing rule. `get_history_models` logs the label
count at `debug!`.

### 3.2 Presets, granularity, limits (confirmed)

| Preset | Range | Auto unit | Overrides offered (≤ 1 000 buckets) |
|---|---|---|---|
| 1h | 3 600 000 | 1m | 1m 5m 15m |
| 6h | 21 600 000 | 5m | 1m 5m 15m 1h |
| 12h | 43 200 000 | 15m | 1m 5m 15m 1h |
| 24h | 86 400 000 | 15m | 5m 15m 1h |
| 7d | 604 800 000 | 1h | 15m 1h 1d |
| 30d | 2 592 000 000 | 1d | 1h 1d |

Units: 1m = 60 000, 5m = 300 000, 15m = 900 000, 1h = 3 600 000,
1d = 86 400 000. A unit is allowed for a preset iff
`rangeMs / ms >= 2` (at least two buckets; this is why 1h/1h and 24h/1d are
out — a one-bucket chart is a number, not a chart) **and**
`ceil(rangeMs / ms) <= 1000`. The table above is exactly what that rule
yields; the only difference from the table Josh accepted is that the 1h
preset no longer offers the 1h unit, which would have been a single bucket.
The override menu lists all five units and disables the disallowed ones, so
the UI never sends a request the server would reject. Server limits are the
backstop, not the UX.

### 3.3 Frontend: pure module `src/lib/history.ts` (tested)

Replaces the day-only helpers in `series.ts` (`HISTORY_DAYS`, `dailyMax`,
`dayLabel`, `axisLabels` are deleted along with their tests;
`polylineRuns`, `seriesDots`, `seriesStats` stay in `series.ts`).

```ts
export type PresetKey = "1h" | "6h" | "12h" | "24h" | "7d" | "30d";
export type UnitKey = "1m" | "5m" | "15m" | "1h" | "1d";
export const PRESETS: Record<PresetKey, { label: string; rangeMs: number; autoUnit: UnitKey }>;
export const PRESET_KEYS: readonly PresetKey[];
export const UNITS: Record<UnitKey, { label: string; ms: number }>;
export const UNIT_KEYS: readonly UnitKey[];
export const MAX_BUCKETS = 1000;

export type Metric = { kind: "week_all" } | { kind: "session" } | { kind: "model"; label: string };
export function metricLabel(m: Metric): string;          // "weekly limit", "session", "<label>"
export function metricKey(m: Metric): string;            // stable string for React keys / equality

export function unitAllowed(preset: PresetKey, unit: UnitKey): boolean;
  // rangeMs / UNITS[unit].ms >= 2 && ceil(rangeMs / ms) <= MAX_BUCKETS
export function effectiveUnit(preset: PresetKey, override: UnitKey | null): UnitKey;
  // override if non-null and allowed, else autoUnit

export function alignedSince(now: number, preset: PresetKey, unit: UnitKey): number;
  // now - rangeMs, then rounded UP to the next unit boundary in LOCAL time
  // (a value already on a boundary stays put):
  //   1m/5m/15m/1h -> next multiple of the unit after the local hour start
  //   1d           -> next local midnight (setHours(0,0,0,0) then +1 day if moved)
  // Rounding up keeps `now - since <= rangeMs`, so a 30d request can never
  // exceed the server's 30-day limit, and bucket `t` values still line up
  // with the axis labels. The partial first unit is dropped, not stretched.

export function bucketCount(since: number, now: number, unit: UnitKey): number;
  // ceil((now - since) / ms), always >= 1 (the last slot is the current, partial unit)

export function nearestKnownSlot(vals: readonly (number | null)[], fraction: number): number | null;
  // fraction in [0,1] across the plot width. polylineRuns places slot i at
  // x = i/(n-1), so the candidate is round(fraction * (n-1)); from there the
  // nearest non-null slot wins (ties to the lower index); null when all null.

export function bucketSeries(points: readonly HistoryPoint[], since: number, unit: UnitKey, count: number): Array<number | null>;
  // slot i = max pct of points whose t falls in [since + i*ms, since + (i+1)*ms); null when none.
  // Points outside [since, since + count*ms) are ignored (defensive; the server already scoped them).

export function axisLabelsFor(since: number, unit: UnitKey, count: number, labels: number, locale?: string): string[];
  // `labels` evenly spaced slot indices (first and last always included).
  // unit 1d or range >= 2 d  -> toLocaleDateString(locale, {month:"short", day:"numeric"})
  // otherwise                -> toLocaleTimeString(locale, {hour:"2-digit", minute:"2-digit"})
  // (range = count * ms)

export function slotLabel(since: number, unit: UnitKey, index: number, count: number, locale?: string): string;
  // tooltip text prefix: date+time for sub-day units, date for 1d, same locale rules

export function missingLabel(missing: number, unit: UnitKey): string;
  // "12 m" | "3 h" | "2 d"  (unit-aware, used by the stats strip: "missing 3 h")
export function showMissing(unit: UnitKey): boolean;
  // false for "1m" (finer than poll density, so gaps are expected), true otherwise
```

DST note (accepted, documented): day buckets are fixed 86 400 000 ms from an
aligned `since`; across a DST change the day boundary shifts by one hour for
the remainder of the range. The bucket value is a maximum, which tolerates a
reading landing one bucket over. `axisLabelsFor`/`slotLabel` label a 1d slot
by `since`'s local date plus `index` days via `setDate`, so labels stay on
calendar days even when the underlying boundary drifts.

### 3.4 Frontend: drawer and data flow

`HistoryDrawer` owns its own fetch. Props become
`{ accountId, latest: SnapshotDto | null, now, cycle, onError, onCollapse }`
where `cycle` is a counter `useDashboard` increments on every
`cycle:finished` (and on mount); `latest` is what the drawer derives its
stroke colour from (below).
State: `preset` (default `"30d"`), `unitOverride: UnitKey | null` (default
`null`), `metric: Metric` (default `{kind:"week_all"}`), `models: string[]`,
`loading`, and one `result: { points, since, unit, count, fetchNow } | null`
written atomically when a request resolves. The chart renders **only** from
`result`, never from the current picker state, so a failed or in-flight
switch (say 30d/1d → 1h/1m) keeps showing the previous series on its own
grid instead of re-bucketing old points against a new one.

Effects:
- On mount: `get_history_models` → `models`. Failure → `onError`, picker shows
  the two base metrics only.
- On `[preset, unitOverride, metric, cycle]` change: compute
  `unit = effectiveUnit`, `since = alignedSince(Date.now(), preset, unit)`,
  invoke `get_history { accountId, since, bucketMs: UNITS[unit].ms, metric }`.
  Requests carry a sequence number; only the latest resolves into state (same
  guard pattern as `useDashboard.load`). Failure → `onError`, previous points
  kept.
- Changing `preset` resets `unitOverride` to `null` if the current override is
  not allowed for the new preset (so the menu never shows an illegal pick).
- If the selected model disappears from `models` after a refetch, the metric
  stays selected (its series may still exist in range) — no automatic reset.

Rendering from `result`: `vals = bucketSeries(points, since, unit, count)`;
`polylineRuns(vals, 100, 100)`; `seriesDots(vals)`; `seriesStats(vals)`.
`fetchNow` is the `Date.now()` captured when the request was built and
`count = bucketCount(since, fetchNow, unit)` is fixed with it, so the slot
grid does not drift while the response is in flight. Hover is one code path:
an `onMouseMove` / `onMouseLeave` pair on the plot maps the pointer's x
fraction to `nearestKnownSlot(vals, fraction)` and shows the tooltip at that
slot (`${slotLabel(...)} · ${value}%`, edge-aware transform as today). The
per-dot mouse handlers go away. Dots themselves are decorative and render
only when `count <= 200` (7d at 1h = 168 has them; 24h at 5m = 288 does
not) so a 1 000-slot series is not 1 000 DOM nodes.

Controls, one row under the drawer header (all `.btn-sm` choices, current one
`choice-on`):
- range: `1h 6h 12h 24h 7d 30d`
- unit: `1m 5m 15m 1h 1d`, disabled when `!unitAllowed`; a tiny "auto" pill
  before the list, on when `unitOverride === null`; clicking a unit sets the
  override, clicking "auto" clears it.
- metric: `<select>` with "weekly limit", "session", then each model label.

Header label: `Last {PRESETS[preset].label} · {metricLabel(metric)} · {unit}`.
Stats strip: `peak N%`, `avg N%`, `missing {missingLabel(missing, unit)}`,
`collapse`. The `missing` stat is shown only when `showMissing(unit)` is
true, i.e. the unit is 5m or longer: poll density is one row per 1.7–3.4 min,
so at 1m roughly half the slots are empty on a perfectly healthy account and
the number would mislead. Axis: `axisLabelsFor(since, unit, count, 4)`.
Y labels unchanged.

Stroke colour: `metricColor(latest value of the selected metric)` — for the
week-all metric it is the same stroke the row uses today; for session it is
the session pct; for a model it is that model's latest pct (0 if absent).
`AccountRow` passes `row.latest` and the drawer derives the stroke itself.

Sparkline: `useDashboard.loadHistoryFor` requests
`{ since: alignedSince(now, "7d", "1h"), bucketMs: 3_600_000, metric: {kind:"week_all"} }`
per account. `AccountRow.last7` is deleted (the request is already 7 days).
The sparkline's hourly-max-with-breaks behaviour is unchanged.

Mock backend: `get_history` and `get_history_models` are implemented against
per-account raw samples (`{t, session, week_all, models: [{label,pct}]}`)
generated every 2 minutes from 09:00 to 18:00 local for the 30 seeded days,
bucketed with the same anchored formula. The mock validates the same limits
and throws `{code:"out_of_range"}` so the drawer's error path can be exercised
in the browser. Model labels per account: `claude3` → `Fable`, `Opus`;
`claude` → `Fable`; `claude2` → `Fable`, `Sonnet`.

## 4. Feature 2 — hideable columns

### 4.1 Prefs

`Prefs` gains `hiddenColumns: ColumnKey[]` (default `[]`).
`parsePrefs`: `normalizeHiddenColumns(v)` in `columns.ts` returns known keys
minus `"account"`, de-duplicated, in the order given; non-array or any
non-string element → `[]` with a `console.warn`. `DEFAULT_PREFS` and `fresh()`
include it. Existing stored records without the field parse to `[]`.

### 4.2 Pure helpers (`columns.ts`, tested)

```ts
export const ALWAYS_VISIBLE: readonly ColumnKey[] = ["account"];
export function visibleColumns(order: readonly ColumnKey[], hidden: readonly ColumnKey[]): ColumnKey[];
  // order minus hidden; "account" can never be removed
export function moveVisible(order: readonly ColumnKey[], hidden: readonly ColumnKey[], from: number, to: number): ColumnKey[];
  // `from`/`to` index the VISIBLE list. Let visible = visibleColumns(order, hidden);
  // fullFrom = order.indexOf(visible[from]); fullTo = order.indexOf(visible[to]);
  // return moveItem(order, fullFrom, fullTo). Only the dragged key moves;
  // hidden keys stay exactly where they are in the full order, so a drag made
  // while columns are hidden (by the user or by the narrow layout) never
  // rearranges columns the user could not see. With nothing hidden this is
  // moveItem. Out-of-range indices return `order` unchanged.
export function gridMinWidth(order: readonly ColumnKey[]): number;
  // Sum of every track's minimum px (the px inside `minmax(Npx, …)` or a bare
  // `Npx`), plus the lead and trail tracks, plus the row-grid gap between
  // tracks. Used by layout tests to prove each layout's table fits its band.
```

### 4.3 Wiring

- `App` computes `visible = visibleColumns(prefs.columnOrder, effectiveHidden)`
  where `effectiveHidden = union(prefs.hiddenColumns, autoHiddenColumns(layout))`
  (§5.2) and passes `visible` as `columnOrder` to `AccountsTable`.
- `AccountsTable` already renders headers, cells and `gridTemplate` from the
  `columnOrder` prop, so it needs no knowledge of hidden columns except at
  drop time: `colUp` calls `onColumnMove(from, to)` instead of computing
  `moveItem` itself, and `App` applies `moveVisible(prefs.columnOrder,
  effectiveHidden, from, to)` and saves the result. Drag geometry
  (`colRects`, `colDragTarget`, `colLineX`) is untouched: it already measures
  the rendered headers.
- Settings gains a **Columns** section between "Text size" and the divider:
  one `.choice` button per key in `DEFAULT_ORDER` order, `choice-on` when
  not in `prefs.hiddenColumns`; the Account button is rendered disabled with
  `title="Account is always shown"`. Clicking toggles membership in
  `prefs.hiddenColumns`. Columns that the narrow layout is currently
  auto-hiding (§5.2) are rendered **disabled** (still showing their pref
  state) with a `hint` line under the row: "Updated and Per model are hidden
  while the window is narrower than 820 px." A control that looks live but
  does nothing is not acceptable.
- The hint line under the table (`<p className="hint">drag a row handle …`)
  is removed from `App.tsx` (Josh, 2026-09-16); the `.hint` class stays for
  the Settings hints.

## 5. Feature 3 — compact view

### 5.1 Window

`src-tauri/tauri.conf.json`: `minWidth: 360`, `minHeight: 240` (from
720×440). Default size unchanged (980×640).

### 5.2 Layout detection (`src/lib/layout.ts`, tested; `src/hooks/useViewport.ts`)

```ts
export type Layout = "full" | "narrow" | "cards";
export const BREAKPOINTS = { narrow: 820, cards: 640 } as const;   // local (unzoomed) px
export const SHELL_PADDING = 95;  // .app 2×22 + .panel border 2×1 + .row-grid padding 2×16 + 17 scrollbar allowance
export function layoutFor(viewportWidthPx: number, zoom: number): Layout;
  // toLocal(width, zoom) < cards -> "cards"; < narrow -> "narrow"; else "full"
export function autoHiddenColumns(layout: Layout): readonly ColumnKey[];
  // full -> []; narrow -> ["updated", "model"]; cards -> [] (table not rendered)
```

Where the numbers come from (this corrects the 660 / 500 Josh accepted,
which I had computed without the grid gaps and paddings): the full table is
8 tracks — lead 26 + account 150 + 3 × 86 + spark 76 + updated 82 + trail 62
= 654 — with 7 gaps × 10 = **724 px** of tracks; around that sit
`.row-grid`'s 2 × 16 px padding, `.panel`'s 2 × 1 px border, `.app`'s
2 × 22 px padding, and a 17 px allowance for WebView2's classic vertical
scrollbar (`window.innerWidth` includes it, the table cannot use it)
(`SHELL_PADDING = 95`), giving 819, so "full" is safe from 820 up. Dropping
Updated and Per model leaves 6 tracks: 26 + 150 + 86 + 86 + 76 + 62 = 486 +
5 × 10 = **536 px** + 95 = 631, so "narrow" is safe from 640 up, and cards
take over below that.
`layout.test.ts` asserts `gridMinWidth(DEFAULT_ORDER) + SHELL_PADDING <=
BREAKPOINTS.narrow` and `gridMinWidth(visibleColumns(DEFAULT_ORDER,
autoHiddenColumns("narrow"))) + SHELL_PADDING <= BREAKPOINTS.cards`, so a
future width change to a column cannot silently reopen the overflow.
`SHELL_PADDING` is built from three CSS values (`.app` padding, `.panel`
border, `.row-grid` padding) plus the fixed 17 px scrollbar allowance; the
constant's comment names all four so a CSS change is traceable to it.

`useViewport()` returns `window.innerWidth`, seeded synchronously and
re-read from a `ResizeObserver` on `document.documentElement` (which fires
on every window resize); the observer is disconnected on unmount.
`innerWidth` includes the vertical scrollbar, so a list that grows tall
enough to scroll cannot flip the layout back and forth at a breakpoint the
way `clientWidth` would. The width is in viewport px; `layoutFor` divides by
the text-size zoom so the breakpoints hold at every size setting. `App`
derives `layout = layoutFor(width, SIZES[prefs.size].zoom)`.

### 5.3 What each layout shows

| Layout | Header | Body |
|---|---|---|
| full | as today | table, user-hidden columns removed |
| narrow | as today | table, user-hidden ∪ {updated, model} removed |
| cards | title and account count hidden; chip, refresh, settings stay; banner bar unchanged | one `AccountCard` per account, in `sort_order` |

Cards layout drops row drag, column drag, the chart drawer and the edit
drawer; widen the window to get them back. Settings still opens (it scrolls;
the Columns section is still shown so hidden prefs can be changed). The
failure pill remains clickable and opens `FailureDetail`. `useDashboard`
still loads the 7-day sparkline series in cards layout (one indexed query
per account per cycle); accepted rather than coupling the hook to layout.

### 5.4 `AccountCard` and `Ring`

`AccountCard` (`src/components/AccountCard.tsx`): a `.card` with a top line
(`acct-dot` coloured by `accountDotColor`, label, status pill when
`pill.tone !== "success"`) and a row of three `Ring`s:

| Ring | value | label |
|---|---|---|
| session | `latest.session.pct` | `session` |
| week | `latest.week_all.pct` | `week` |
| top model | `summarizeModels(latest.week_models).pct` | the model label (`title` lists all) |

`Ring` (`src/components/Ring.tsx`, geometry in `src/lib/gauge.ts`, tested):

```ts
export const RING = { size: 44, stroke: 5, radius: 19.5 } as const;
export function ringDash(pct: number | null, radius: number): { circumference: number; offset: number };
  // circumference = 2πr; offset = circumference * (1 - clamp(pct,0,100)/100); null -> offset = circumference
```

Rendering per the dataviz method: single-value magnitude → a meter. Track
circle in `var(--track-off)`, arc circle rotated −90° so it starts at 12
o'clock, `stroke-linecap: round`, arc colour `metricColor(pct)` (the same
status thresholds as the linear `Meter`), value centred in the mono text
colour (`#dbe1e6`, not the series colour), label under in `var(--muted)`.
`null` → track only, centre "—". `ringDash` clamps to 0..100 (the DTO already
clamps, so this is defensive, never a second lap). `role="img"` with
`aria-label="{label} {pct}%"`.
The card is `display:flex; flex-wrap:wrap; gap: 10px 14px` so three 44 px
rings plus labels fit at 360 px minus padding; at 300 px they wrap to 2+1.

### 5.5 Always on top

> 2026-09-17: the "Keep window on top" toggle moved from Settings to an
> "on top" button in the header, present in every layout.

`Prefs.alwaysOnTop: boolean` (default `false`; `parsePrefs` accepts only a
boolean). `Backend` gains `setAlwaysOnTop(flag: boolean): Promise<void>`;
real = `getCurrentWindow().setAlwaysOnTop(flag)` from
`@tauri-apps/api/window`; mock = `console.info` no-op. Capability
`core:window:allow-set-always-on-top` is added to
`src-tauri/capabilities/default.json`. `App` runs an effect on
`[prefs.alwaysOnTop]` that awaits the call and routes failure to `showError`.
Settings gains a `Toggle` "Keep window on top" (hint: "stay visible over
other apps") in the toggles block, wired to `onPrefs({ alwaysOnTop })`.

## 6. Remove the default-account flag

- **Rust:** drop `is_default` from `Account` (`usage/mod.rs`), from
  `accounts.rs` (the column list, `insert_account`, `build_account`, the row
  mapper, `ORDER BY sort_order ASC, lower(label) ASC, label ASC`), from the
  `account()` test helper in `tray.rs`, and from the seed path:
  `seed_accounts_if_empty(candidates, now)` sorts candidates by
  case-insensitive label only. `lib.rs` stops computing `default_dir`;
  `paths::default_config_dir` and its tests are deleted (it has no other
  caller). Tests asserting `is_default` are rewritten to assert order only.
- **Schema:** `SCHEMA_VERSION = 3`; `V3: ALTER TABLE accounts DROP COLUMN
  is_default;` in its own transaction with its version bump. **The V2 step
  currently bumps `user_version` to `{SCHEMA_VERSION}` via `format!`, not to
  a literal 2** — left as is, a fresh DB would jump V1 → V2 → version 3 and
  skip V3, keeping a `NOT NULL` column with no default that the new
  `insert_account` no longer fills, so the first seed or add would fail.
  Each step bumps to its own literal (`user_version=2`, `user_version=3`);
  `SCHEMA_VERSION` is only the target the tests compare against. V1/V2 SQL
  is otherwise historical and untouched (a fresh DB still passes through it;
  the V2 backfill's `ORDER BY is_default DESC` runs before V3 drops the
  column). No index or trigger references `is_default`. Tests: fresh DB ends
  at 3 with no `is_default` in `PRAGMA table_info(accounts)` **and** can
  insert an account; a DB migrated from V2 with rows keeps every row and its
  `sort_order`.
- **Frontend:** `Account.is_default` removed from `types.ts`; the `default`
  tag and `.tag` CSS removed; `mockBackend.makeAccount` loses `isDefault`.
- **Docs:** `docs/2026-09-15-claude-usage-tracker-design.md` D17 / §6 / §8 get
  a one-line "2026-09-16: `is_default` removed" note where they mention it;
  the ui-overhaul plan's "make default" deviation row is left as history.

## 7. Error handling

- Every backend call in new code goes through a try/catch that routes to
  `onError` (drawer, settings toggle, always-on-top effect) or, for the
  sparkline load, `setError` in `useDashboard`, exactly as today.
- The drawer keeps its last good series on failure and shows the toast.
- `ResizeObserver` is guarded: if unavailable (`typeof ResizeObserver ===
  "undefined"`), `useViewport` returns the initial width and logs once at
  `console.warn`.
- Server rejections are `out_of_range` with a message naming the argument.
  A well-behaved UI reaches them in one way: a request built more than five
  minutes before the command ran (suspend, or a long poll cycle holding the
  blocking pool). The drawer then shows the toast, keeps its last result,
  and the next `cycle` tick rebuilds the request with a fresh `since`. Any
  other rejection means `history.ts` and the Rust limits disagree, which the
  constants test and the `unitAllowed` sweep guard.

## 8. Testing

Four gates on every task: `cargo test --manifest-path src-tauri/Cargo.toml`,
`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`,
`npm test`, `npm run build`. TDD throughout: tests first, red, then green.

Rust (`snapshots.rs`, `commands.rs`, `schema.rs`, `accounts.rs`):
- bucketing anchored at `since` for each metric; empty buckets absent; rows
  with `outcome != 'ok'` ignored; model metric matches label exactly and
  ignores other models; `history_models` returns distinct sorted labels
  within retention only.
- every validation rule rejects with `OutOfRange` at the boundary and accepts
  one step inside it (bucket 59 999 vs 60 000; 1 001 vs 1 000 buckets; range
  30 d + 5 min + 1 ms vs 30 d + 5 min; `since > now`; label empty / 65 chars).
- malformed `week_models` JSON on one row does not fail the model query or
  `history_models`; a REAL-valued `pct` is rounded.
- V3 migration (fresh DB: version 3, no column, insert works; V2 DB with rows:
  rows and `sort_order` preserved); seed order by label; no `is_default`
  anywhere.

TypeScript (`history.test.ts`, `columns.test.ts`, `prefs.test.ts`,
`layout.test.ts`, `gauge.test.ts`, `series.test.ts` trimmed):
- preset table matches §3.2 exactly (auto unit and allowed set per preset).
- the TypeScript limits come from `historyLimits.json` (asserted equal to
  the imported values), and the Rust test in `snapshots.rs` asserts the same
  file equals the Rust constants.
- `alignedSince` lands on local boundaries (minute, quarter, hour, midnight)
  for 40 `now` values spread across a year (so any local DST transition is
  covered), never exceeds the preset range, and `bucketCount` never exceeds
  1 000 for any allowed pair.
- `bucketSeries` puts a point in the right slot, keeps max, leaves gaps null,
  ignores out-of-range points.
- axis/slot labels: time for sub-day, date for ≥ 2 d, first and last always
  present; `missingLabel` unit words; `showMissing` false only for 1m.
- `nearestKnownSlot` maps `fraction` via `round(fraction * (n-1))`, skips
  nulls, picks the nearer neighbour, ties to the lower index.
- `visibleColumns` never drops `account`; `moveVisible` equals `moveItem`
  with nothing hidden, leaves hidden keys in place when a hidden key precedes
  visible index 0, and a drag made in the narrow auto-hidden set followed by
  "widening" (re-filtering with nothing hidden) shows only the dragged key
  moved.
- `normalizeHiddenColumns` drops `account`, unknowns, duplicates.
- `parsePrefs` defaults `hiddenColumns` and `alwaysOnTop` independently.
- `layoutFor` at 819/820/639/640 with zoom 1 and 1.22; `autoHiddenColumns`;
  `gridMinWidth` + `SHELL_PADDING` fits each band (see §5.2).
- `ringDash` at null/0/50/100/140.

Visual (Playwright MCP against the mock, screenshots attached to the PR):
980 px full table; 720 px narrow (Updated and Per-model gone); 420 px cards;
drawer at 1h/1m, 24h/15m, 7d/1h, 30d/1d and one model metric; Settings with
Session hidden then restored; always-on-top toggle flips without error.
Then one `npm run tauri dev` run to confirm the real window honours 360×240
and the set-always-on-top capability.

## 9. Out of scope

- Persisting the drawer's preset/unit/metric across sessions.
- A custom absolute date range.
- Per-model series on the sparkline or the compact card.
- Light theme.
