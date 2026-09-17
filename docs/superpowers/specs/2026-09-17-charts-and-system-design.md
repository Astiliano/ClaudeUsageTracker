# Continuous charts, machine CPU/memory, week reset, 24-hour sparkline — design

Date: 2026-09-17 · Branch: `charts-and-system` · Follows PR #4 (gate
reconciliation, Claude process usage, stay-on-top button).

Four independent changes to the Claude Usage Tracker, one PR. Decisions
confirmed with Josh on 2026-09-17: the system line shows whole-machine
figures only (the Claude processes' memory and CPU share are dropped; the
count stays); the row sparkline covers the last 24 h at 15-minute buckets;
a week window without a reset time reads "no reset"; the drawer's "missing"
stat is removed.

## 1. Goal

1. **Continuous charts.** Every chart (the row sparkline and the history
   drawer) draws one line through all known points. An empty bucket is
   skipped, never drawn as zero and never a break in the line. Hover dots
   and the peak/avg strip stay; the "missing" stat goes.
2. **Machine CPU and memory.** The system line's two rings become the whole
   machine's CPU busy share and memory used share, in percent ("cpu 12%",
   "mem 41%"), with the absolute figures in the tooltips. The Claude process
   count stays where PR #4 put it (chip or line by `countPlacement`).
3. **Week reset time.** The Week meter's note is the time until the weekly
   reset, exactly as the Session meter shows its own, instead of the
   constant "all models".
4. **Last 24 hours.** The row sparkline shows the last 24 h at 15 m buckets
   instead of 7 d at 1 h. The drawer's presets are unchanged.

## 2. Facts this design relies on (verified 2026-09-17)

- The row sparkline is `src/lib/sparkline.ts` `buildSparklinePaths(points,
  w, h): string[]`, rendered by `Sparkline.tsx` as one `<path>` per string.
  It sorts by `t`, maps x from the first to the last point, and starts a new
  sub-path wherever two consecutive points are more than 1 h apart. It does
  **not** use `polylineRuns`.
- The drawer is `HistoryDrawer.tsx`: `bucketSeries` → `Array<number | null>`,
  then `polylineRuns(vals, 100, 100): string[]` (one `<polyline>` per run of
  non-null slots), `seriesDots` (rendered only when `vals.length <= 200`),
  `seriesStats` → `{ peak, avg, missing }`. `missing === vals.length` drives
  the "no data in this range" caption. Hover is `nearestKnownSlot` over the
  plot, independent of the dots. `missingLabel` / `showMissing` in
  `history.ts` exist only for the strip's "missing 3 h".
- The 2026-09-16 spec §2 chose "empty buckets are breaks, never zeros"
  because snapshots stop while Claude is idle. The zeros half of that rule
  is still right; the breaks half is what this design retires.
- `src-tauri/src/system.rs`: `Sampler` owns a `System`; the first `sample`
  primes with `refresh_cpu_specifics(CpuRefreshKind::nothing())`,
  `refresh_memory()`, two process refreshes with `with_cpu().with_memory()`
  and two `MINIMUM_CPU_UPDATE_INTERVAL` (200 ms) sleeps. `aggregate` sums
  `ProcView.rss_bytes` / `.cpu` over `Exclusion::counts`. `ProcView` carries
  `rss_bytes` and `cpu` for that sum only; the gate's probe (`process.rs`
  line 104) refreshes with `nothing().with_exe(OnlyIfNotSet).with_cmd(OnlyIfNotSet)`
  and reads `name`, `parent`, `start_time` and `cmd` from it, so that refresh
  kind populates everything `Exclusion::counts` needs.
- sysinfo 0.39.6, Windows: `refresh_cpu_usage()` opens a PDH query on first
  use (slow once), collects it, and sets `global_cpu_usage()` to
  `100 - % Idle Time`. A rate counter needs two collections at least
  `MINIMUM_CPU_UPDATE_INTERVAL` apart, so the first collection reads 0 and
  the second is a true diff. It touches only the CPU counters, never the
  per-process baselines. `refresh_memory()` is one `GlobalMemoryStatusEx`
  call; `used_memory()` is total minus available.
- `SystemReport { stats: Option<SystemStats>, stopped }` (`commands.rs`) is
  a clone of `Core.system`; the frontend mirrors it in `types.ts`.
  `useSystem` and the `system:sampled` event are shape-agnostic.
- `Header.tsx` derives the chip count as `system?.stats?.claude.count ?? null`.
  `SystemLine.tsx` maps `systemLine()`'s items to markup and draws a `sm`
  `Ring` for the `cpu` and `mem` keys only.
- `systemLine` (`src/lib/system.ts`) is a pure state table, rows 1–8, one
  vitest per row in `system.test.ts`.
- `weekNote(pct)` returns `{ text: "all models" | "at limit", warn }`, warn
  at `THRESHOLDS.crit` (95). `AccountRow`'s `week` case renders
  `week === null ? "no data" : n.text`. `sessionNote(session, now)` returns
  "no data" / "idle" / `formatLeft(resets_at, now)`. `Win { pct, resets_at:
  number | null }` types both `session` and `week_all`, so the week reset is
  already on the wire. `AccountCard` labels its rings "session", "week",
  and the model label; none shows a countdown.
- `useDashboard.loadHistoryFor` requests `alignedSince(now, "7d", "1h")` at
  `UNITS["1h"].ms`, metric `WEEK_ALL`, once per cycle per account.
  `COLUMNS.spark.label` is "7 days" at 76 px; `Sparkline` has
  `aria-label="weekly usage, last 7 days"`. `unitAllowed("24h", "15m")` is
  true (the 24h preset's auto unit); `bucketCount` for it is 96 or 97.
- Gates: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
  `npm test`, `npm run build`. Vitest is node-only on `src/lib/*`.
  `VITE_MOCK_BACKEND=1 npm run dev` plus the Playwright MCP for visual checks.
  Serena is Rust-only here; TypeScript is read natively.

## 3. Change 1 — continuous charts

### 3.1 `src/lib/series.ts`

`polylineRuns` is replaced by

```ts
export function seriesLine(vals: readonly (number | null)[], width: number, height: number): string;
  // One SVG `points` string through every known value, at x = i/(n-1)*width
  // (0 when n === 1) and y = height - clamp(v,0,100)/100*height, two decimals
  // as today. Null slots are skipped: the segment runs straight from the last
  // known value to the next. "" when there is no known value.
```

`seriesDots` and `seriesStats` are unchanged, including `missing`, which the
drawer still uses for the empty-range caption. The `polylineRuns` tests are
rewritten for `seriesLine`: through-a-gap (`[0, null, 100]` →
`"0.00,24.00 100.00,0.00"`), leading and trailing nulls, single known value,
empty, all-null, clamping.

### 3.2 `src/lib/sparkline.ts`

`buildSparklinePaths` becomes

```ts
export function buildSparklinePath(points: HistoryPoint[], width: number, height: number): string | null;
  // null for no points. Otherwise one path: `M x0 y0 L x1 y1 …` through the
  // points sorted by t, x from the first to the last point as today, y as
  // today. A single point is `M x y L x y` (a zero-length line the round caps
  // render as a dot). No gap rule.
```

Tests: the two gap tests ("breaks the line at a gap", "never emits a zero
for a missing hour") become one, "draws straight through a gap" (four points
with a three-hour hole → one path, four coordinates, no y of `height`); the
others (rising y, inside the box, single point, lone point at the right
edge, sorting) are kept against the new signature.

`Sparkline.tsx` renders one `<path>` (or the `—` placeholder when null);
`aria-label` changes with change 4 (§6).

### 3.3 `HistoryDrawer.tsx`

- `lines` becomes `line: string` from `seriesLine`; one `<polyline>` when
  `line !== ""`.
- Dots render when `dots.length <= MAX_DOTS` (known values, not slots), so a
  sparse long range still shows its few points and a dense short one does
  not pay for 1 000 nodes. A single known value in a range is therefore
  always visible as a dot, even though a one-point polyline draws nothing.
- Stats strip: `peak N%`, `avg N%`, `collapse`. `missingLabel` and
  `showMissing` are deleted from `history.ts` with their test; the drawer's
  imports follow. "no data in this range" is unchanged.
- The `nearestKnownSlot` doc comment stops naming `polylineRuns`.

### 3.4 Documentation

- `docs/superpowers/specs/2026-09-16-history-and-compact-design.md` §2, the
  "breaks, never zeros" fact, gains a dated note: since 2026-09-17 the line is
  continuous through empty buckets, which are skipped, never zeros; §3.3 gets
  a note that `missingLabel` / `showMissing` were removed the same day; §3.4's
  stats-strip sentence and its "hourly-max-with-breaks" sparkline sentence
  get the same note.
- `docs/2026-09-15-claude-usage-tracker-design.md` §7 row description
  (line 670): "sparkline (15-minute max week-all pct, last 24 hours; empty
  buckets are skipped, never zeros; the line is continuous)".

## 4. Change 2 — machine CPU and memory

### 4.1 Data (`src-tauri/src/system.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemStats {
    /// Epoch milliseconds, so the UI can tell a live figure from a frozen one.
    pub sampled_at: i64,
    /// Whole-machine CPU busy share, 0..=100 (100 − PDH "% Idle Time").
    pub cpu_pct: f32,
    /// Whole-machine physical memory in use (total − available), bytes.
    pub mem_used_bytes: u64,
    pub mem_total_bytes: u64,
    /// Claude Code processes other than the poll child and any child of this app.
    pub claude_count: u32,
}
```

`ClaudeStats` is deleted. `ProcView` loses `rss_bytes` and `cpu` (both
readers now need only `pid`, `parent`, `start_time`, `name`, `cmd`); its
`From<&Process>` and the `process.rs` test fixture follow.

Pure, tested:

- `count_claude(procs: &[ProcView], exclusion: &Exclusion) -> u32` replaces
  `aggregate`: the number of views for which `exclusion.counts(view)`.
- `cpu_share(raw: f32) -> f32`: `0.0` when `!raw.is_finite()`, else
  `raw.clamp(0.0, 100.0)`. PDH can hand back a NaN on a counter hiccup;
  `f32::clamp` would pass it through.
- `presence_edge` and `after_panic` unchanged.

`Sampler { system, self_pid, self_started_at, pid_slot, primed }` (no
`cpus`, no cached `mem_total_bytes`). `PROCESS_REFRESH` becomes
`nothing().with_exe(OnlyIfNotSet).with_cmd(OnlyIfNotSet)` — the gate probe's
kind; no CPU or memory per process.

`sample(&mut self, now_ms) -> Sampled` (`Sampled { stats, elapsed_ms, did_prime }`
unchanged):

- First call only (`!primed`):
  1. `refresh_cpu_usage()` — opens the PDH query and takes the first
     collection (reads 0; discarded).
  2. `PROCESS_REFRESH`; read `self_started_at` from
     `process(self_pid).start_time()` (0 if absent, as today).
  3. `std::thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL)`; set `primed`.
- Every call: `refresh_cpu_usage()`, `refresh_memory()`, `PROCESS_REFRESH`;
  then `SystemStats { sampled_at: now_ms, cpu_pct: cpu_share(global_cpu_usage()),
  mem_used_bytes: used_memory(), mem_total_bytes: total_memory(),
  claude_count: count_claude(&views, &exclusion) }`.

The first published CPU figure is the second PDH collection, 200 ms after
the first, so it is a true interval average, and the priming costs one sleep
instead of two (the per-process CPU baseline that needed the second one is
gone). Memory is re-read every sample because `used_memory` changes; the
total is read with it for free. Cost per sample: one PDH collection, one
`GlobalMemoryStatusEx`, one process walk without per-process CPU or memory
reads — less than today.

`run_sampler` is unchanged except for the log fields (§8) and the field
names (`stats.claude_count`). `Core`, `SystemSlot`, `lock_system`,
`EventSink::system_sampled`, `SystemReport` and `core_get_system` keep their
shapes; only the `SystemStats` payload changes.

### 4.2 Frontend data (`src/lib/types.ts`, `src/lib/system.ts`)

`types.ts`: `SystemStats { sampled_at: number; cpu_pct: number; mem_used_bytes:
number; mem_total_bytes: number; claude_count: number }`; `ClaudeStats` is
deleted; `SystemReport` unchanged.

`system.ts` keeps `formatBytes`, `isStale`, `clock`, `processCountSuffix`,
`SAMPLE_INTERVAL_MS`, `STALE_AFTER_MS`; changes:

```ts
export function memPct(stats: SystemStats): number | null;
  // mem_used_bytes / mem_total_bytes * 100 clamped 0..100; null when total is 0
export interface SysItem { key: "cpu" | "mem" | "count" | "waiting" | "unavailable"; pct: number | null; text: string; title: string }
```

The `none` key is gone: a zero count no longer replaces the machine figures.
State table (rows 1–3 exclusive, first match wins; row 4 otherwise; rows 5–7
dim modifiers on row 4, first true one supplies the reason appended to every
item title as today):

| # | condition | items | dimmed |
|---|---|---|---|
| 1 | `stats === null && error !== null` | unavailable (title = error) | no |
| 2 | `stats === null && stopped` | unavailable (title "sampler stopped, see log") | no |
| 3 | `stats === null` | waiting | no |
| 4 | stats present | cpu, mem, [count when `showCount`] | rows 5–7 |
| 5 | `error !== null` | as 4 | yes, the error |
| 6 | `stopped` | as 4 | yes, "sampler stopped, see log" |
| 7 | `isStale(stats, now)` | as 4 | yes, "last sample HH:MM:SS" |

Items:

- cpu: `pct = cpu_pct`, text `cpu 12%` (rounded), title
  `machine CPU: 12% busy`.
- mem: `pct = memPct`, text `mem 41%`, title
  `machine memory: 13.1 GB of 32.0 GB used (41%)` (both through
  `formatBytes`); when `memPct` is null: text `mem —`, title
  `machine memory: total unknown`.
- count (only when `showCount`): `pct = null`, text `2 procs` / `1 proc` /
  `0 procs`, title `Claude Code processes running`.

`Header.tsx`: `const count = system?.stats?.claude_count ?? null;`. `chipFor`,
`countPlacement`, `processCountSuffix` are unchanged, so with the count on
the chip a zero count still appends nothing (the idle chip already says
"waits for Claude Code").

### 4.3 Rendering (`SystemLine.tsx`)

Unchanged apart from the group's `aria-label`, which becomes
`"system usage"`; the `sm` ring still draws for `cpu` and `mem` only, the
text still wears the chip ink, and the line still dims via `sysline-stale`.
CSS is untouched.

Header at full width:

```
Usage Tracker  3 accounts        ● polling every 300 s · 2 Claude processes  [refresh] [on top] [settings]
◔ cpu 12%   ◑ mem 41%
```

cards:

```
● idle · waits for Claude Code   [refresh] [on top] [settings]
◔ cpu 12%   ◑ mem 41%   2 procs
```

### 4.4 Mock backend

`get_system` returns `sampled_at: Date.now()`, `cpu_pct` drifting 5–30,
`mem_used_bytes ≈ 13 GiB ± 3 %`, `mem_total_bytes = 32 GiB`,
`claude_count: 2`. `?mockSystem=stopped|error` behave as today.

### 4.5 Documentation

`docs/superpowers/specs/2026-09-17-gate-and-system-design.md` §4 is the
design of record for the system line, so it is revised in place rather than
annotated: a "Revised 2026-09-17 (charts-and-system)" line under the §4
heading, then §4.1 (data, `count_claude`, `cpu_share`, the one-sleep priming),
§4.4 (types, the seven-row table, the items), §4.5 (aria-label, header
sketches), §4.6 (mock), §7 (log fields), §8 (tests) and §10 (the
whole-machine bullet is removed; a bullet "per-process CPU or memory
figures (removed 2026-09-17)" takes its place) are rewritten to match this
section. §4.2, §4.3 and the presence logic are untouched.
`docs/2026-09-15-claude-usage-tracker-design.md` §6.2's sampler paragraph
says what is sampled (machine CPU, machine memory, Claude count).

## 5. Change 3 — week reset time

`present.ts`:

```ts
export function weekNote(week: Win | null, now: number): { text: string; warn: boolean };
  // null           -> { text: "no data",                  warn: false }
  // resets_at null -> { text: "no reset",                 warn: pct >= THRESHOLDS.crit }
  // otherwise      -> { text: formatLeft(resets_at, now), warn: pct >= THRESHOLDS.crit }
```

"no reset" is distinct from "no data": the CLI reported a week percentage
without a reset clause, and saying "no data" would make a real reading look
like a missed poll. The warn tone stays on the note when the percentage is at
the limit; it costs one boolean that already exists.

`AccountRow`'s `week` case: `const n = weekNote(week, now); <Meter pct={week?.pct ?? null} note={n.text} noteWarn={n.warn} />`.
`sessionNote` is unchanged. `AccountCard` is unchanged: its ring labels name
the metric, and the session ring does not show a countdown either.

Tests (`present.test.ts`): null → "no data" not warn; `{pct: 96, resets_at: null}`
→ "no reset", warn; `{pct: 40, resets_at: now + 2 d 3 h}` → `formatLeft`'s
text, not warn; `{pct: 95, resets_at: now + 1 h}` → warn.

## 6. Change 4 — last 24 hours

`history.ts`:

```ts
export const SPARK_PRESET: PresetKey = "24h";
export const SPARK_UNIT: UnitKey = "15m";
```

with a test that `unitAllowed(SPARK_PRESET, SPARK_UNIT)` holds and that
`bucketCount(alignedSince(now, SPARK_PRESET, SPARK_UNIT), now, SPARK_UNIT)`
is at most 97 for a few `now` values. `useDashboard.loadHistoryFor` requests
`alignedSince(now, SPARK_PRESET, SPARK_UNIT)` at `UNITS[SPARK_UNIT].ms`; its
doc comment says "24 hours of 15-minute week-all maxima".

`COLUMNS.spark.label` becomes `"24 hours"` (76 px is unchanged; the header
font fits eight characters). `Sparkline`'s `aria-label` becomes
`"weekly limit, last 24 hours"`. The mock's samples (09:00–18:00 local on
seeded days) always intersect a trailing 24 h window, so the row sparkline
has data in the mock at any time of day.

Documentation: `docs/2026-09-15-claude-usage-tracker-design.md` §8
`get_history` row: "The sparkline asks for 24 h / 15 min week-all once per
cycle"; §7 row description per §3.4. `docs/superpowers/specs/2026-09-16-history-and-compact-design.md`
§2 (`AccountRow` filters to 7 days) and §3.4 (the sparkline request) get a
dated note: 24 h / 15 m since 2026-09-17.

## 7. Error handling

- Sampler: unchanged loop; a panicking sample still rebuilds after one
  interval, three in a row still stop it. `cpu_share` turns a non-finite PDH
  value into 0 rather than publishing NaN (which `serde_json` would reject).
  `total_memory() == 0` yields `mem —` through `memPct`'s null, never a
  division by zero.
- `useSystem`: unchanged; a rejected `get_system` dims the line with the
  error as title.
- Charts: an all-null series renders no line, no dots, and the "no data in
  this range" caption as today; a single known value renders one dot (drawer)
  or a zero-length round-capped path (sparkline).
- Week note: `formatLeft` already handles a past `resets_at` ("now").

## 8. Logging

- INFO `claude processes changed {count}` (no `rss_bytes`).
- DEBUG `system sample {elapsed_ms, count, cpu_pct, mem_used_bytes, did_prime}`.
- Everything else (`presence wake`, the ERROR lines) unchanged.

## 9. Testing

Four gates on every task: `cargo test --manifest-path src-tauri/Cargo.toml`,
`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`,
`npm test`, `npm run build`. TDD throughout.

Rust:

- `system.rs`: `count_claude` counts only the views the exclusion accepts and
  is 0 for empty input; `cpu_share` at NaN, −1, 150, 12.3; `presence_edge`
  and `after_panic` tests kept. Smoke test `sample_primes_once_and_returns_usable_figures`
  asserts `did_prime` true then false, `mem_total_bytes > 0`,
  `mem_used_bytes <= mem_total_bytes`, and `cpu_pct` finite within 0..=100
  on both calls.
- `process.rs`: the `Exclusion::counts` tests and fixtures build `ProcView`
  without `rss_bytes` / `cpu`.
- `commands.rs`: `core_get_system_reports_no_stats_before_the_first_sample_then_the_sample_then_stopped`
  builds the new `SystemStats`.

TypeScript:

- `series.test.ts`: `seriesLine` per §3.1; `seriesDots` / `seriesStats` tests kept.
- `sparkline.test.ts`: per §3.2.
- `history.test.ts`: `missingLabel` / `showMissing` test removed; `SPARK_PRESET`
  / `SPARK_UNIT` test added.
- `system.test.ts`: `memPct` on used/total (0 total → null, over-total →
  100); `systemLine` one test per row of the §4.2 table; item texts and
  titles (`cpu 12%`, `mem 41%`, the absolute figures, `mem —` for a 0
  total); `0 procs`, `1 proc`, `2 procs` under `showCount`; no count item
  without it; `isStale`, `clock`, `processCountSuffix`, `formatBytes` kept.
- `present.test.ts`: `weekNote` per §5; `chipFor` / `countPlacement` kept.
- `columns.test.ts` (if it pins labels): "24 hours".

Visual (Playwright MCP against the mock, screenshots attached to the PR):
980 px full table showing continuous row sparklines labelled "24 hours", the
system line "cpu N% · mem N%" with the count on the chip, and a Week note
reading "Nd Nh left"; one open drawer at 7d/1h with a continuous line, dots
and a strip of peak · avg · collapse; 420 px cards with the count in the
line; `?mockSystem=error`. Then one `npm run tauri dev` run (release instance
stopped, relaunched after) to confirm the cpu and mem figures move and are
plausible against Task Manager, and that the chip count still tracks open
Claude sessions.

## 10. Out of scope

- Any change to the drawer's presets, units or hover.
- Per-process CPU or memory figures (removed here; the count remains).
- Anchoring the row sparkline's x axis to the requested window rather than
  to the first and last point (today's mapping is kept).
- The two live gate checks left open from PR #4 (only Josh can run them).
