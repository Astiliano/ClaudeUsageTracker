# Continuous charts, machine CPU/memory, week reset, 24-hour sparkline — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every chart a continuous line, switch the header's system line to whole-machine CPU % and memory %, show the weekly reset countdown under the Week meter, and shrink the row sparkline to the last 24 hours at 15-minute buckets.

**Architecture:** Four independent changes on one branch. Charts are pure SVG-string builders in `src/lib` rendered by thin components. The system line is a Rust sampler (`sysinfo`) publishing `SystemStats` through `get_system`, mirrored in `types.ts`, decided by the pure `systemLine` state table. The week note and sparkline window are one-line pure helpers plus their call sites. Every rule that needs a test is a pure function in `src/lib/` or `src-tauri/src/`.

**Tech Stack:** Rust (Tauri 2, sysinfo 0.39.6, tokio, serde), TypeScript + React, Vite, vitest (node-only, `src/**/*.test.ts`), Playwright MCP for visual checks against `VITE_MOCK_BACKEND=1 npm run dev`.

**Spec:** `docs/superpowers/specs/2026-09-17-charts-and-system-design.md` (read it first; every task cites its section).

## Global Constraints

- Work in the session checkout on branch `charts-and-system`. Never create or enter a git worktree.
- TDD on every task: write the failing test, run it red, implement, run it green, run the full gate, commit.
- Four gates, all must pass before a task is committed (run the ones the task touches; run all four before the PR):
  - `cargo test --manifest-path src-tauri/Cargo.toml`
  - `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
  - `npm test`
  - `npm run build`
- No `any`, no non-null `!` assertions, no silent catches. Every timer and listener is cleaned up (none are added here).
- Serena is Rust-only in this repo; read TypeScript with native tools.
- Copy rules (verbatim from the spec): texts `cpu 12%`, `mem 41%`, `mem —`, `2 procs` / `1 proc` / `0 procs`; titles `machine CPU: 12% busy`, `machine memory: 13.1 GB of 32.0 GB used (41%)`, `machine memory: total unknown`, `Claude Code processes running`; week note `no data` / `no reset` / `formatLeft(...)`; column label `24 hours`; sparkline `aria-label="weekly limit, last 24 hours"`; system line group `aria-label="system usage"`.
- Commit messages end with:
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_019mZN481MkHraAvBpuDroGU
  ```
- Never run `/usage` through Git Bash. Do not touch the running release instance of the app unless a task says so.

---

## File map

| File | Responsibility | Task |
|---|---|---|
| `src/lib/series.ts` (+ `.test.ts`) | slot-indexed drawer series → one `points` string, dots, stats | 1 |
| `src/lib/history.ts` (+ `.test.ts`) | presets/units; loses `missingLabel`/`showMissing`; gains `SPARK_PRESET`/`SPARK_UNIT` | 1, 6 |
| `src/components/HistoryDrawer.tsx` | renders one polyline, dots by known count, strip without "missing" | 1 |
| `src/lib/sparkline.ts` (+ `.test.ts`) | time-indexed row series → one path string | 2 |
| `src/components/Sparkline.tsx` | one `<path>`; aria-label | 2, 6 |
| `src-tauri/src/process.rs` | `ProcView` without `rss_bytes`/`cpu` | 3 |
| `src-tauri/src/system.rs` | `SystemStats` (machine figures + count), `count_claude`, `cpu_share`, one-sleep priming | 3 |
| `src-tauri/src/commands.rs` | `core_get_system` test fixture | 3 |
| `src/lib/types.ts` | `SystemStats` mirror | 4 |
| `src/lib/system.ts` (+ `.test.ts`) | `memPct`, `systemLine` seven-row table | 4 |
| `src/components/SystemLine.tsx`, `src/components/Header.tsx`, `src/lib/mockBackend.ts` | aria-label, `claude_count`, mock stats | 4 |
| `src/lib/present.ts` (+ `.test.ts`), `src/components/AccountRow.tsx` | `weekNote(week, now)` | 5 |
| `src/hooks/useDashboard.ts`, `src/lib/columns.ts` | 24 h / 15 m request, "24 hours" label | 6 |
| `docs/superpowers/specs/2026-09-16-history-and-compact-design.md`, `docs/superpowers/specs/2026-09-17-gate-and-system-design.md`, `docs/2026-09-15-claude-usage-tracker-design.md` | dated notes and the §4 rewrite | 7 |

---

### Task 1: Drawer draws one continuous line; "missing" stat removed

**Files:**
- Modify: `src/lib/series.ts` (replace `polylineRuns`)
- Modify: `src/lib/series.test.ts`
- Modify: `src/lib/history.ts` (delete `missingLabel`, `showMissing`; fix the `nearestKnownSlot` comment)
- Modify: `src/lib/history.test.ts` (delete their test and imports)
- Modify: `src/components/HistoryDrawer.tsx`

**Interfaces:**
- Produces: `seriesLine(vals: readonly (number | null)[], width: number, height: number): string` in `src/lib/series.ts`. `seriesDots`, `seriesStats` unchanged.
- Spec: §3.1, §3.3.

- [ ] **Step 1: Rewrite the `polylineRuns` tests as `seriesLine` tests**

Replace the `describe("polylineRuns", …)` block in `src/lib/series.test.ts` with:

```ts
describe("seriesLine", () => {
  it("draws straight through a null slot instead of breaking", () => {
    expect(seriesLine([0, null, 100], 100, 24)).toBe("0.00,24.00 100.00,0.00");
  });
  it("skips leading and trailing nulls but keeps the slot positions", () => {
    // n = 4, so slots 1 and 2 sit at x = 33.33 and 66.67.
    expect(seriesLine([null, 10, 20, null], 100, 24)).toBe("33.33,21.60 66.67,19.20");
  });
  it("places a single known value at its slot", () => {
    expect(seriesLine([50], 100, 24)).toBe("0.00,12.00");
    expect(seriesLine([null, 50], 100, 24)).toBe("100.00,12.00");
  });
  it("is empty for an empty series or a series with no known values", () => {
    expect(seriesLine([], 100, 24)).toBe("");
    expect(seriesLine([null, null], 100, 24)).toBe("");
  });
  it("clamps values into 0..100", () => {
    expect(seriesLine([150, -10], 100, 100)).toBe("0.00,0.00 100.00,100.00");
  });
});
```

and change the import to `import { seriesDots, seriesLine, seriesStats } from "./series";`. Leave the `seriesDots / seriesStats` block as it is.

- [ ] **Step 2: Run the file, expect failure**

Run: `npx vitest run src/lib/series.test.ts`
Expected: FAIL — `seriesLine` is not exported.

- [ ] **Step 3: Implement `seriesLine`**

In `src/lib/series.ts`, delete `polylineRuns` (function and doc comment) and add in its place:

```ts
/**
 * One SVG `points` string through every known value. Slot i sits at
 * x = i/(n-1) * width (0 when n === 1); a null slot is skipped, so the line
 * runs straight from the last known value to the next one. Empty buckets
 * are gaps in sampling, not readings, so they are neither zeros nor breaks.
 * "" when nothing is known.
 */
export function seriesLine(
  vals: readonly (number | null)[],
  width: number,
  height: number,
): string {
  const n = vals.length;
  const points: string[] = [];
  vals.forEach((v, i) => {
    if (v === null) return;
    const x = n === 1 ? 0 : (i / (n - 1)) * width;
    const y = height - (clamp(v, 0, 100) / 100) * height;
    points.push(`${x.toFixed(2)},${y.toFixed(2)}`);
  });
  return points.join(" ");
}
```

- [ ] **Step 4: Run the file, expect green**

Run: `npx vitest run src/lib/series.test.ts`
Expected: PASS (5 + 3 tests).

- [ ] **Step 5: Delete `missingLabel` / `showMissing` and their test**

In `src/lib/history.ts` delete the `missingLabel` and `showMissing` functions (and their doc comments). In the `nearestKnownSlot` doc comment replace `polylineRuns places slot i` with `seriesLine places slot i`.

In `src/lib/history.test.ts` delete the test `"missing label is unit aware and the stat is hidden at 1m"` (the whole `it(...)` block) and remove `missingLabel,` and `showMissing,` from the import list.

- [ ] **Step 6: Wire the drawer**

In `src/components/HistoryDrawer.tsx`:

1. Import line from `../lib/history`: remove `missingLabel,` and `showMissing,`.
2. Import from `../lib/series`: `import { seriesDots, seriesLine, seriesStats } from "../lib/series";`
3. Replace the `useMemo` block with:

```ts
  // Derived once per result, not per mouse move (a 720-slot series would
  // otherwise be re-bucketed and re-stringified on every pointer event).
  const { vals, stats, line, dots, axis } = useMemo(() => {
    const v: Array<number | null> = result === null ? [] : bucketSeries(result.points, result.since, result.unit, result.count);
    const known = seriesDots(v);
    return {
      vals: v,
      stats: seriesStats(v),
      line: seriesLine(v, 100, 100),
      // Dots are decorative: skipped when there are many KNOWN values, so a
      // sparse long range still shows its few points.
      dots: known.length <= MAX_DOTS ? known : [],
      axis: result === null ? [] : axisLabelsFor(result.since, result.unit, result.count, AXIS_LABELS),
    };
  }, [result]);
```

4. Update the `MAX_DOTS` comment to: `/** Dots are decorative; above this many known values they are skipped (hover still works). */`
5. Replace the `{lines.map(...)}` polyline block with:

```tsx
          {line !== "" && (
            <polyline points={line} fill="none" stroke={stroke} strokeWidth={1.8} vectorEffect="non-scaling-stroke" strokeLinejoin="round" strokeLinecap="round" />
          )}
```

6. In the stats strip delete the line `{showMissing(shownUnit) && <span>missing {missingLabel(stats.missing, shownUnit)}</span>}`. Keep `peak`, `avg`, `collapse` and the `stats.missing === vals.length` empty caption.

- [ ] **Step 7: Full TS gate**

Run: `npm test && npm run build`
Expected: both pass; `tsc` reports no unused imports.

- [ ] **Step 8: Commit**

```bash
git add src/lib/series.ts src/lib/series.test.ts src/lib/history.ts src/lib/history.test.ts src/components/HistoryDrawer.tsx
git commit -m "Drawer: one continuous line through known buckets, drop the missing stat"
```

---

### Task 2: Row sparkline draws one continuous path

**Files:**
- Modify: `src/lib/sparkline.ts` (replace `buildSparklinePaths`)
- Modify: `src/lib/sparkline.test.ts`
- Modify: `src/components/Sparkline.tsx`

**Interfaces:**
- Produces: `buildSparklinePath(points: HistoryPoint[], width: number, height: number): string | null`.
- Spec: §3.2.

- [ ] **Step 1: Rewrite the tests**

Replace the whole body of `describe("buildSparklinePaths", …)` in `src/lib/sparkline.test.ts` with a `describe("buildSparklinePath", …)` block (import becomes `import { buildSparklinePath } from "./sparkline";`; keep `HOUR`, `BASE`, `pts`):

```ts
describe("buildSparklinePath", () => {
  it("returns null for an empty series", () => {
    expect(buildSparklinePath([], 100, 20)).toBeNull();
  });

  it("returns one path for contiguous hours", () => {
    const path = buildSparklinePath(pts([[0, 10], [1, 20], [2, 30]]), 100, 20);
    expect(path).not.toBeNull();
    expect(path?.match(/L /g)).toHaveLength(2);
  });

  it("draws straight through a gap instead of breaking or dropping to zero", () => {
    // Hours 0 and 1, a three-hour hole, then hours 5 and 6: still one path.
    const path = buildSparklinePath(pts([[0, 10], [1, 20], [5, 30], [6, 40]]), 100, 20);
    expect(path).not.toBeNull();
    expect(path?.startsWith("M ")).toBe(true);
    expect(path?.match(/L /g)).toHaveLength(3);
    // y = 20 is pct 0; nothing in this series is 0.
    expect(path).not.toMatch(/ 20(?:\s|$)/);
  });

  it("maps a higher percentage to a smaller y so the line rises", () => {
    const path = buildSparklinePath(pts([[0, 0], [1, 100]]), 100, 20) ?? "";
    const ys = [...path.matchAll(/-?\d+(?:\.\d+)?\s+(-?\d+(?:\.\d+)?)/g)].map((m) => Number(m[1]));
    expect(ys[0]).toBeGreaterThan(ys[1]);
  });

  it("keeps every point inside the box", () => {
    const path = buildSparklinePath(pts([[0, 0], [1, 50], [2, 100]]), 120, 24) ?? "";
    const numbers = [...path.matchAll(/(-?\d+(?:\.\d+)?)/g)].map((m) => Number(m[1]));
    for (let i = 0; i < numbers.length; i += 2) {
      expect(numbers[i]).toBeGreaterThanOrEqual(0);
      expect(numbers[i]).toBeLessThanOrEqual(120);
      expect(numbers[i + 1]).toBeGreaterThanOrEqual(0);
      expect(numbers[i + 1]).toBeLessThanOrEqual(24);
    }
  });

  it("renders a single point as a zero-length line at the right edge", () => {
    const path = buildSparklinePath(pts([[3, 42]]), 100, 20);
    expect(path).toMatch(/^M [\d.]+ [\d.]+ L [\d.]+ [\d.]+$/);
    expect(path?.startsWith("M 100 ")).toBe(true);
  });

  it("sorts unordered input before drawing", () => {
    const unordered = pts([[2, 30], [0, 10], [1, 20]]);
    const ordered = pts([[0, 10], [1, 20], [2, 30]]);
    expect(buildSparklinePath(unordered, 100, 20)).toEqual(buildSparklinePath(ordered, 100, 20));
  });
});
```

- [ ] **Step 2: Run red**

Run: `npx vitest run src/lib/sparkline.test.ts`
Expected: FAIL — `buildSparklinePath` is not exported.

- [ ] **Step 3: Implement**

Replace everything in `src/lib/sparkline.ts` below the `import type { HistoryPoint }` line (including the old `HOUR` constant) with:

```ts
function round(n: number): number {
  return Math.round(n * 100) / 100;
}

/**
 * Hand-rolled SVG path data for the row sparkline: one continuous path
 * through every point, sorted by time, x from the first to the last point.
 * Empty buckets are gaps in sampling (Claude was idle), not readings, so
 * they are neither drawn as zeros nor allowed to break the line. Null when
 * there is nothing to draw.
 */
export function buildSparklinePath(
  points: HistoryPoint[],
  width: number,
  height: number,
): string | null {
  if (points.length === 0) {
    return null;
  }

  const sorted = [...points].sort((a, b) => a.t - b.t);
  const first = sorted[0].t;
  const last = sorted[sorted.length - 1].t;
  const span = last - first;

  const x = (t: number): number =>
    span === 0 ? width : round(((t - first) / span) * width);
  const y = (pct: number): number =>
    round(height - (Math.min(Math.max(pct, 0), 100) / 100) * height);

  const head = sorted[0];
  const start = `M ${x(head.t)} ${y(head.pct)}`;
  if (sorted.length === 1) {
    // A lone reading still has to be visible: a zero-length line that the
    // round caps render as a dot.
    return `${start} L ${x(head.t)} ${y(head.pct)}`;
  }
  const rest = sorted
    .slice(1)
    .map((p) => `L ${x(p.t)} ${y(p.pct)}`)
    .join(" ");
  return `${start} ${rest}`;
}
```

- [ ] **Step 4: Run green**

Run: `npx vitest run src/lib/sparkline.test.ts`
Expected: PASS (7 tests).

- [ ] **Step 5: Render one path**

Replace the body of `Sparkline` in `src/components/Sparkline.tsx`:

```tsx
import type { JSX } from "react";
import { buildSparklinePath } from "../lib/sparkline";
import type { HistoryPoint } from "../lib/types";

interface Props {
  points: HistoryPoint[];
  stroke: string;
}

/** Hand-rolled SVG, no chart library: one continuous path through every bucket. */
export function Sparkline({ points, stroke }: Props): JSX.Element {
  const d = buildSparklinePath(points, 100, 24);

  if (d === null) {
    return <span className="spark-empty">—</span>;
  }

  return (
    <svg
      viewBox="0 0 100 24"
      preserveAspectRatio="none"
      role="img"
      aria-label="weekly usage, last 7 days"
    >
      <path
        d={d}
        fill="none"
        stroke={stroke}
        strokeWidth={1.6}
        strokeLinecap="round"
        strokeLinejoin="round"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}
```

(The `aria-label` is changed in Task 6, not here.)

- [ ] **Step 6: TS gate**

Run: `npm test && npm run build`
Expected: pass.

- [ ] **Step 7: Commit**

```bash
git add src/lib/sparkline.ts src/lib/sparkline.test.ts src/components/Sparkline.tsx
git commit -m "Row sparkline: one continuous path through every bucket"
```

---

### Task 3: Sampler publishes machine CPU %, machine memory and the Claude count

**Files:**
- Modify: `src-tauri/src/process.rs` (`ProcView` loses `rss_bytes`, `cpu`; `From` impl; test fixture `view`)
- Modify: `src-tauri/src/system.rs`
- Modify: `src-tauri/src/commands.rs` (the `core_get_system_…` test and its import)

**Interfaces:**
- Produces (Rust, `crate::system`): `SystemStats { sampled_at: i64, cpu_pct: f32, mem_used_bytes: u64, mem_total_bytes: u64, claude_count: u32 }`; `count_claude(&[ProcView], &Exclusion) -> u32`; `cpu_share(f32) -> f32`. `ClaudeStats` and `aggregate` are deleted. Serialized field names are the frontend contract for Task 4.
- Spec: §4.1, §7, §8, §9 (Rust).

- [ ] **Step 1: Trim `ProcView`**

In `src-tauri/src/process.rs`:
- Delete `pub rss_bytes: u64,` and `pub cpu: f32,` from `struct ProcView`.
- Delete `rss_bytes: p.memory(),` and `cpu: p.cpu_usage(),` from the `From<&sysinfo::Process>` impl.
- In the tests' `fn view(...)`, delete `rss_bytes: 0,` and `cpu: 0.0,`.

- [ ] **Step 2: Write the new `system.rs` tests (red)**

Replace the `mod tests` in `src-tauri/src/system.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{Exclusion, ProcView};

    fn view(pid: u32, name: &str) -> ProcView {
        ProcView {
            pid,
            parent: Some(1),
            start_time: 9_000,
            name: name.to_string(),
            cmd: vec![name.to_string()],
        }
    }

    fn exclusion() -> Exclusion {
        Exclusion { self_pid: 100, self_started_at: 5_000, poll_child: None }
    }

    #[test]
    fn count_claude_counts_only_the_views_the_exclusion_accepts() {
        let procs = vec![view(200, "claude.exe"), view(201, "claude.exe"), view(202, "code.exe")];
        assert_eq!(count_claude(&procs, &exclusion()), 2);
    }

    #[test]
    fn count_claude_of_nothing_is_zero() {
        assert_eq!(count_claude(&[], &exclusion()), 0);
    }

    #[test]
    fn cpu_share_clamps_and_never_publishes_a_non_finite_value() {
        assert_eq!(cpu_share(f32::NAN), 0.0, "a PDH hiccup must not become NaN on the wire");
        assert_eq!(cpu_share(f32::INFINITY), 0.0);
        assert_eq!(cpu_share(-1.0), 0.0);
        assert_eq!(cpu_share(150.0), 100.0);
        assert_eq!(cpu_share(12.3), 12.3);
    }

    #[test]
    fn presence_edge_fires_only_on_zero_to_non_zero() {
        assert!(!presence_edge(0, 0));
        assert!(presence_edge(0, 1));
        assert!(!presence_edge(1, 2));
        assert!(!presence_edge(2, 0));
    }

    #[test]
    fn after_panic_waits_a_full_interval_then_gives_up_on_the_third() {
        assert_eq!(after_panic(1), Some(SAMPLE_INTERVAL));
        assert_eq!(after_panic(2), Some(SAMPLE_INTERVAL));
        assert_eq!(after_panic(3), None, "three in a row stops the sampler");
    }

    /// Smoke test against the real machine: it must return, prime on the
    /// first call only, and produce usable machine figures. Not a value
    /// assertion; the numbers depend on the host.
    #[test]
    fn sample_primes_once_and_returns_usable_figures() {
        let mut sampler = Sampler::new(Arc::new(AtomicU32::new(0)));
        let first = sampler.sample(1_700_000_000_000);
        assert!(first.did_prime, "the first call runs the priming collection");
        assert!(first.stats.mem_total_bytes > 0);
        assert!(first.stats.mem_used_bytes <= first.stats.mem_total_bytes);
        assert!(first.stats.cpu_pct.is_finite());
        assert!((0.0..=100.0).contains(&first.stats.cpu_pct));

        let second = sampler.sample(1_700_000_005_000);
        assert!(!second.did_prime, "priming happens once per Sampler");
        assert!((0.0..=100.0).contains(&second.stats.cpu_pct));
    }
}
```

- [ ] **Step 3: Run red**

Run: `cargo test --manifest-path src-tauri/Cargo.toml system::`
Expected: compile errors — `count_claude`, `cpu_share` undefined; `ProcView` fields.

- [ ] **Step 4: Rewrite the data and the sampler**

In `src-tauri/src/system.rs`:

Imports: `use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};` (drop `CpuRefreshKind`).

Replace `ClaudeStats` and `SystemStats` with:

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

Replace `aggregate` with:

```rust
/// How many of the views are Claude Code processes we count (spec §4.1 of
/// the gate-and-system design: the exclusion drops the poll child and this
/// app's own children).
pub fn count_claude(procs: &[ProcView], exclusion: &Exclusion) -> u32 {
    procs
        .iter()
        .filter(|view| exclusion.counts(view))
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

/// A publishable CPU share. PDH can hand back a non-finite value on a
/// counter hiccup and `f32::clamp` would pass NaN through, so it is 0 here.
pub fn cpu_share(raw: f32) -> f32 {
    if raw.is_finite() {
        raw.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

/// The gate probe's refresh kind: enough for name, parent, start time and
/// command line, no per-process CPU or memory. (The spec calls this
/// `PROCESS_REFRESH`; sysinfo's builder methods are not `const fn`, so it is
/// a function.)
fn process_refresh() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_exe(UpdateKind::OnlyIfNotSet)
        .with_cmd(UpdateKind::OnlyIfNotSet)
}
```

Replace `struct Sampler`, `Sampler::new`, `refresh_processes` and `sample` with:

```rust
pub struct Sampler {
    system: System,
    self_pid: u32,
    self_started_at: u64,
    pid_slot: Arc<AtomicU32>,
    primed: bool,
}

impl Sampler {
    pub fn new(pid_slot: Arc<AtomicU32>) -> Sampler {
        Sampler {
            system: System::new(),
            self_pid: std::process::id(),
            self_started_at: 0,
            pid_slot,
            primed: false,
        }
    }

    fn refresh_processes(&mut self) {
        self.system
            .refresh_processes_specifics(ProcessesToUpdate::All, true, process_refresh());
    }

    pub fn sample(&mut self, now_ms: i64) -> Sampled {
        let started = Instant::now();
        let did_prime = !self.primed;

        if did_prime {
            // 1. Open the PDH query and take its first collection.
            //    "% Idle Time" is a rate counter: this first read fails
            //    inside sysinfo and surfaces as 100 % busy, so it is never
            //    published; the collection below is the first real one.
            self.system.refresh_cpu_usage();
            // 2. Our own start time, for the exclusion's recycled-pid clause.
            self.refresh_processes();
            self.self_started_at = self
                .system
                .process(Pid::from_u32(self.self_pid))
                .map(|p| p.start_time())
                .unwrap_or(0);
            // 3. The second collection must be at least this far from the first.
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            self.primed = true;
        }

        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.refresh_processes();
        let views: Vec<ProcView> = self
            .system
            .processes()
            .values()
            .map(ProcView::from)
            .collect();
        let exclusion = Exclusion {
            self_pid: self.self_pid,
            self_started_at: self.self_started_at,
            poll_child: match self.pid_slot.load(Ordering::SeqCst) {
                0 => None,
                p => Some(p),
            },
        };

        Sampled {
            stats: SystemStats {
                sampled_at: now_ms,
                cpu_pct: cpu_share(self.system.global_cpu_usage()),
                mem_used_bytes: self.system.used_memory(),
                mem_total_bytes: self.system.total_memory(),
                claude_count: count_claude(&views, &exclusion),
            },
            elapsed_ms: started.elapsed().as_millis() as u64,
            did_prime,
        }
    }
}
```

In `run_sampler`, replace the post-sample block from `if presence_edge(` down to `prev_count = …;` with:

```rust
        let Sampled { stats, elapsed_ms, did_prime } = sampled;
        if presence_edge(prev_count, stats.claude_count) {
            info!("presence wake");
            core.triggers.presence();
        }
        if prev_count != stats.claude_count {
            info!(count = stats.claude_count, "claude processes changed");
        }
        debug!(
            elapsed_ms,
            count = stats.claude_count,
            cpu_pct = stats.cpu_pct,
            mem_used_bytes = stats.mem_used_bytes,
            did_prime,
            "system sample"
        );
        prev_count = stats.claude_count;
```

Update the module doc comment on `run_sampler` if it mentions memory sums (it says "Samples every 5 s, publishes into `Core.system`, and fires the presence trigger" — fine as is).

- [ ] **Step 5: Fix the `commands.rs` test**

In `src-tauri/src/commands.rs` tests: change `use crate::system::{ClaudeStats, SystemStats};` to `use crate::system::SystemStats;` and the fixture in `core_get_system_reports_no_stats_before_the_first_sample_then_the_sample_then_stopped` to:

```rust
        let stats = SystemStats {
            sampled_at: 1_700_000_000_000,
            cpu_pct: 12.5,
            mem_used_bytes: 13 * 1024 * 1024 * 1024,
            mem_total_bytes: 32 * 1024 * 1024 * 1024,
            claude_count: 2,
        };
```

- [ ] **Step 6: Rust gates**

Run: `cargo test --manifest-path src-tauri/Cargo.toml && cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
Expected: all tests pass (including the `process.rs` exclusion tests and the smoke test), clippy clean. If clippy objects to `.try_into().unwrap_or(u32::MAX)` on the `usize`, replace it with `u32::try_from(...).unwrap_or(u32::MAX)` (same saturating meaning).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/process.rs src-tauri/src/system.rs src-tauri/src/commands.rs
git commit -m "Sampler: machine CPU and memory shares plus the Claude count; drop per-process figures"
```

---

### Task 4: Frontend system line shows machine CPU % and memory %

**Files:**
- Modify: `src/lib/types.ts`
- Modify: `src/lib/system.ts`
- Modify: `src/lib/system.test.ts`
- Modify: `src/components/SystemLine.tsx` (aria-label)
- Modify: `src/components/Header.tsx` (`claude_count`)
- Modify: `src/lib/mockBackend.ts` (`systemStats`)

**Interfaces:**
- Consumes: the serialized `SystemStats` from Task 3 (`sampled_at`, `cpu_pct`, `mem_used_bytes`, `mem_total_bytes`, `claude_count`).
- Produces: `SysItem.key: "cpu" | "mem" | "count" | "waiting" | "unavailable"`; `memPct(stats)`; `systemLine(...)` seven-row table.
- Spec: §4.2, §4.3, §4.4, §9 (TypeScript).

- [ ] **Step 1: Types**

In `src/lib/types.ts` delete `ClaudeStats` and make:

```ts
export interface SystemStats {
  sampled_at: number;
  /** Whole-machine CPU busy share, 0..100. */
  cpu_pct: number;
  mem_used_bytes: number;
  mem_total_bytes: number;
  claude_count: number;
}
```

- [ ] **Step 2: Rewrite the tests (red)**

In `src/lib/system.test.ts`:

Replace the `stats` helper with:

```ts
function stats(over: Partial<SystemStats> = {}): SystemStats {
  return {
    sampled_at: 1_000_000,
    cpu_pct: 12.4,
    mem_used_bytes: Math.round(13.1 * GIB),
    mem_total_bytes: 32 * GIB,
    claude_count: 2,
    ...over,
  };
}
```

Replace the `memPct` block with:

```ts
describe("memPct", () => {
  it("is used over total, clamped", () => {
    expect(memPct(stats())).toBeCloseTo(40.94, 1);
    expect(memPct(stats({ mem_total_bytes: 0 }))).toBeNull();
    expect(memPct(stats({ mem_total_bytes: GIB, mem_used_bytes: 4 * GIB }))).toBe(100);
  });
});
```

Replace the `systemLine state table` block's rows 4–8 and the outranking test with:

```ts
  it("row 4: stats give cpu and mem in percent, with the absolute figures as titles", () => {
    const line = systemLine({ report: report(stats()), error: null, showCount: false, now: NOW });
    expect(line.items.map((i) => i.key)).toEqual(["cpu", "mem"]);
    expect(line.items[0]).toMatchObject({ pct: 12.4, text: "cpu 12%", title: "machine CPU: 12% busy" });
    expect(line.items[1].pct).toBeCloseTo(40.94, 1);
    expect(line.items[1].text).toBe("mem 41%");
    expect(line.items[1].title).toBe("machine memory: 13.1 GB of 32.0 GB used (41%)");
    expect(line.dimmed).toBe(false);
  });

  it("row 4: the count item appears only when asked, with singular and zero forms", () => {
    const two = systemLine({ report: report(stats()), error: null, showCount: true, now: NOW });
    expect(two.items.map((i) => i.key)).toEqual(["cpu", "mem", "count"]);
    expect(two.items[2]).toMatchObject({ pct: null, text: "2 procs", title: "Claude Code processes running" });
    expect(systemLine({ report: report(stats({ claude_count: 1 })), error: null, showCount: true, now: NOW }).items[2].text).toBe("1 proc");
    expect(systemLine({ report: report(stats({ claude_count: 0 })), error: null, showCount: true, now: NOW }).items[2].text).toBe("0 procs");
  });

  it("row 4: a zero count still shows the machine figures", () => {
    const line = systemLine({ report: report(stats({ claude_count: 0 })), error: null, showCount: false, now: NOW });
    expect(line.items.map((i) => i.key)).toEqual(["cpu", "mem"]);
  });

  it("row 4: an unknown total is a dash", () => {
    const line = systemLine({ report: report(stats({ mem_total_bytes: 0 })), error: null, showCount: false, now: NOW });
    expect(line.items[1]).toMatchObject({ pct: null, text: "mem —", title: "machine memory: total unknown" });
  });

  it("row 5: an error dims live stats and supplies the reason", () => {
    const line = systemLine({ report: report(stats()), error: "boom", showCount: false, now: NOW });
    expect(line.items.map((i) => i.key)).toEqual(["cpu", "mem"]);
    expect(line.dimmed).toBe(true);
    expect(line.items[0].title.endsWith("boom")).toBe(true);
  });

  it("row 6: stopped dims live stats at once, before they turn stale", () => {
    const line = systemLine({ report: report(stats(), true), error: null, showCount: false, now: NOW });
    expect(line.dimmed).toBe(true);
    expect(line.items[0].title.endsWith("sampler stopped, see log")).toBe(true);
  });

  it("row 7: a stale sample dims and names its time", () => {
    // 2026-09-17 14:02:11 local, so the clock is pinned without a locale.
    const at = new Date(2026, 8, 17, 14, 2, 11).getTime();
    const line = systemLine({
      report: report(stats({ sampled_at: at })),
      error: null,
      showCount: false,
      now: at + STALE_AFTER_MS + 1,
    });
    expect(line.dimmed).toBe(true);
    expect(line.items[0].title.endsWith("last sample 14:02:11")).toBe(true);
  });

  it("an error outranks stopped and staleness", () => {
    const line = systemLine({
      report: report(stats(), true),
      error: "boom",
      showCount: false,
      now: NOW + STALE_AFTER_MS + 1,
    });
    expect(line.items[0].title.endsWith("boom")).toBe(true);
  });
```

Rows 1–3 and the `formatBytes`, `isStale`, `clock`, `processCountSuffix` blocks stay as they are.

- [ ] **Step 3: Run red**

Run: `npx vitest run src/lib/system.test.ts`
Expected: FAIL (type errors on the fixture and wrong texts).

- [ ] **Step 4: Implement `system.ts`**

In `src/lib/system.ts`:

Replace `memPct`:

```ts
/** The machine's memory in use as a share of the total, or null without a total. */
export function memPct(stats: SystemStats): number | null {
  if (stats.mem_total_bytes === 0) return null;
  const pct = (stats.mem_used_bytes / stats.mem_total_bytes) * 100;
  return Math.max(0, Math.min(100, pct));
}
```

Change `SysItem.key` to `"cpu" | "mem" | "count" | "waiting" | "unavailable"`.

Replace `liveItems` with:

```ts
function liveItems(stats: SystemStats, showCount: boolean): SysItem[] {
  const cpu = Math.round(stats.cpu_pct);
  const mem = memPct(stats);
  const items: SysItem[] = [
    { key: "cpu", pct: stats.cpu_pct, text: `cpu ${cpu}%`, title: `machine CPU: ${cpu}% busy` },
    mem === null
      ? { key: "mem", pct: null, text: "mem —", title: "machine memory: total unknown" }
      : {
          key: "mem",
          pct: mem,
          text: `mem ${Math.round(mem)}%`,
          title: `machine memory: ${formatBytes(stats.mem_used_bytes)} of ${formatBytes(stats.mem_total_bytes)} used (${Math.round(mem)}%)`,
        },
  ];
  if (showCount) {
    items.push({
      key: "count",
      pct: null,
      text: countText(stats.claude_count),
      title: "Claude Code processes running",
    });
  }
  return items;
}
```

In `systemLine`, replace the `const items: SysItem[] = stats.claude.count === 0 ? [...] : liveItems(stats, showCount);` expression with `const items = liveItems(stats, showCount);` and update the doc comment to: "Rows 1 to 3 are exclusive; row 4 is any stats; rows 5 to 7 are dim modifiers on row 4, the first true one supplying the reason."

- [ ] **Step 5: Components and mock**

- `src/components/SystemLine.tsx`: `aria-label="system usage"`; doc comment: "The second header line: the machine's CPU and memory shares, and the Claude process count when the chip does not carry it."
- `src/components/Header.tsx`: `const count = system?.stats?.claude_count ?? null;`
- `src/lib/mockBackend.ts`: replace `systemStats` with

```ts
  const systemStats = (): SystemStats => ({
    sampled_at: Date.now(),
    cpu_pct: Math.round((5 + Math.random() * 25) * 10) / 10,
    mem_used_bytes: Math.round(13 * GIB * (0.97 + Math.random() * 0.06)),
    mem_total_bytes: 32 * GIB,
    claude_count: 2,
  });
```

- [ ] **Step 6: TS gate**

Run: `npm test && npm run build`
Expected: pass; `tsc` must not report a leftover `ClaudeStats` reference anywhere (grep `ClaudeStats src` returns nothing).

- [ ] **Step 7: Commit**

```bash
git add src/lib/types.ts src/lib/system.ts src/lib/system.test.ts src/components/SystemLine.tsx src/components/Header.tsx src/lib/mockBackend.ts
git commit -m "System line: machine CPU and memory in percent, count unchanged"
```

---

### Task 5: Week meter shows the time until the weekly reset

**Files:**
- Modify: `src/lib/present.ts` (`weekNote`)
- Modify: `src/lib/present.test.ts`
- Modify: `src/components/AccountRow.tsx` (the `week` case)

**Interfaces:**
- Produces: `weekNote(week: Win | null, now: number): { text: string; warn: boolean }`.
- Spec: §5.

- [ ] **Step 1: Tests (red)**

Replace the `weekNote flags the limit` test in `src/lib/present.test.ts` with:

```ts
  it("weekNote counts down to the reset, names a missing reset, and warns at the limit", () => {
    const now = 1_000_000;
    const DAY = 86_400_000;
    const HOUR = 3_600_000;
    expect(weekNote(null, now)).toEqual({ text: "no data", warn: false });
    expect(weekNote({ pct: 96, resets_at: null }, now)).toEqual({ text: "no reset", warn: true });
    expect(weekNote({ pct: 40, resets_at: null }, now)).toEqual({ text: "no reset", warn: false });
    expect(weekNote({ pct: 40, resets_at: now + 2 * DAY + 3 * HOUR }, now)).toEqual({ text: "2d 3h left", warn: false });
    expect(weekNote({ pct: 95, resets_at: now + HOUR }, now)).toEqual({ text: "1h 0m left", warn: true });
  });
```

- [ ] **Step 2: Run red**

Run: `npx vitest run src/lib/present.test.ts`
Expected: FAIL (signature mismatch).

- [ ] **Step 3: Implement**

In `src/lib/present.ts` replace `weekNote`:

```ts
/**
 * The Week meter's note: the time until the weekly reset, exactly as the
 * Session meter shows its own. "no reset" is a real reading whose line had
 * no reset clause; "no data" is no reading at all.
 */
export function weekNote(week: Win | null, now: number): { text: string; warn: boolean } {
  if (week === null) return { text: "no data", warn: false };
  const warn = week.pct >= THRESHOLDS.crit;
  if (week.resets_at === null) return { text: "no reset", warn };
  return { text: formatLeft(week.resets_at, now), warn };
}
```

- [ ] **Step 4: Run green**

Run: `npx vitest run src/lib/present.test.ts`
Expected: PASS (`formatLeft` renders sub-day spans as `Nh Mm left` and longer ones as `Nd Nh left`; do not change `formatLeft`).

- [ ] **Step 5: Wire `AccountRow`**

In `src/components/AccountRow.tsx` replace the `week` case with:

```tsx
      case "week": { const n = weekNote(week, now); return <Meter pct={week?.pct ?? null} note={n.text} noteWarn={n.warn} />; }
```

`weekPct` stays (it feeds `stroke`).

- [ ] **Step 6: TS gate**

Run: `npm test && npm run build`
Expected: pass.

- [ ] **Step 7: Commit**

```bash
git add src/lib/present.ts src/lib/present.test.ts src/components/AccountRow.tsx
git commit -m "Week meter: time until the weekly reset instead of 'all models'"
```

---

### Task 6: Row sparkline covers the last 24 hours at 15-minute buckets

**Files:**
- Modify: `src/lib/history.ts` (add `SPARK_PRESET`, `SPARK_UNIT`)
- Modify: `src/lib/history.test.ts`
- Modify: `src/hooks/useDashboard.ts` (`loadHistoryFor`)
- Modify: `src/lib/columns.ts` (`COLUMNS.spark.label`)
- Modify: `src/components/Sparkline.tsx` (`aria-label`)

**Interfaces:**
- Produces: `SPARK_PRESET: PresetKey = "24h"`, `SPARK_UNIT: UnitKey = "15m"` in `src/lib/history.ts`.
- Spec: §6.

- [ ] **Step 1: Test (red)**

Add to `src/lib/history.test.ts` (import `SPARK_PRESET, SPARK_UNIT` alongside the other names):

```ts
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
```

(`NOW`, `MIN`, `HOUR` are the file's existing constants.)

- [ ] **Step 2: Run red**

Run: `npx vitest run src/lib/history.test.ts`
Expected: FAIL — names not exported.

- [ ] **Step 3: Implement**

In `src/lib/history.ts`, directly after `export const WEEK_ALL …` (or after the `UNITS` block if `WEEK_ALL` is elsewhere), add:

```ts
/** The row sparkline's window: the last 24 hours at the 24h preset's own unit. */
export const SPARK_PRESET: PresetKey = "24h";
export const SPARK_UNIT: UnitKey = "15m";
```

In `src/hooks/useDashboard.ts`:

```ts
import { SPARK_PRESET, SPARK_UNIT, UNITS, WEEK_ALL, alignedSince } from "../lib/history";
…
  /** 24 hours of 15-minute week-all maxima per account, for the row sparklines. */
  const loadHistoryFor = useCallback(async (accountIds: string[]): Promise<void> => {
    const now = Date.now();
    const since = alignedSince(now, SPARK_PRESET, SPARK_UNIT);
    …
            bucketMs: UNITS[SPARK_UNIT].ms,
```

In `src/lib/columns.ts`: `spark: { label: "24 hours", width: "76px" },`.

In `src/components/Sparkline.tsx`: `aria-label="weekly limit, last 24 hours"`.

- [ ] **Step 4: TS gate**

Run: `npm test && npm run build`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add src/lib/history.ts src/lib/history.test.ts src/hooks/useDashboard.ts src/lib/columns.ts src/components/Sparkline.tsx
git commit -m "Row sparkline: last 24 hours at 15-minute buckets"
```

---

### Task 7: Documentation

**Files:**
- Modify: `docs/superpowers/specs/2026-09-16-history-and-compact-design.md`
- Modify: `docs/superpowers/specs/2026-09-17-gate-and-system-design.md`
- Modify: `docs/2026-09-15-claude-usage-tracker-design.md`

**Interfaces:** none. Spec: §3.4, §4.5, §6 (documentation paragraphs). No gate other than reading the result back; docs-only commit.

- [ ] **Step 1: 2026-09-16 spec, dated notes**

- §2, the bullet ending `empty buckets are **breaks, never zeros**.` — append: ` *(2026-09-17: the line is now continuous through empty buckets, which are skipped, never zeros; see `2026-09-17-charts-and-system-design.md` §3.)*`
- §2, the bullet containing `AccountRow filters to 7 days for the sparkline` — append: ` *(2026-09-17: the row sparkline requests 24 h at 15 m; see the charts-and-system design §6.)*`
- §3.3, after the `showMissing` signature block's closing fence — add a line: `*(2026-09-17: `missingLabel` and `showMissing` were removed with the "missing" stat; the strip is peak · avg · collapse.)*`
- §3.4, the sentence `Stats strip: … collapse.` and the following `The missing stat is shown only when …` sentences — append after them: ` *(2026-09-17: the "missing" stat was removed.)*`
- §3.4, the paragraph beginning `Sparkline: useDashboard.loadHistoryFor requests` — append: ` *(2026-09-17: now `alignedSince(now, "24h", "15m")` at 900 000 ms, one continuous path; see the charts-and-system design §3.2 and §6.)*`

- [ ] **Step 2: 2026-09-17 gate-and-system spec, §4 rewritten in place**

Under the `## 4. Changes 2 and 3 — Claude process usage and count` heading insert the line:

`> **Revised 2026-09-17 (charts-and-system).** The system line now shows whole-machine CPU and memory shares; the Claude processes' RSS and CPU share were removed and only the count remains. §4.1, §4.4–§4.6, §7, §8 and §10 below describe the revised state; §4.2 and §4.3 are unchanged. The original per-process design is in git history (PR #4).`

Then rewrite these parts to match `docs/superpowers/specs/2026-09-17-charts-and-system-design.md`:
- §4.1: the `ClaudeStats`/`SystemStats` code block becomes the new `SystemStats` (§4.1 of the charts spec); the `ProcView` sentence loses `rss_bytes: u64, cpu: f32`; the `aggregate` bullet becomes `count_claude` and a `cpu_share` bullet; the `Sampler {…}` struct loses `cpus` and `mem_total_bytes`; the "First call only" list becomes the three-step one-sleep priming; the "Every call" line becomes `refresh_cpu_usage()`, `refresh_memory()`, `PROCESS_REFRESH`, then the `SystemStats` literal; the paragraph "A Claude process that appears mid-run reads 0 CPU…" is deleted; the cost paragraph says one PDH collection, one `GlobalMemoryStatusEx`, one process walk.
- §4.4: `types.ts` line → the new `SystemStats`, no `ClaudeStats`; the state table → the seven-row table from the charts spec §4.2; the `system.ts` signature block → `memPct` on used/total, `SysItem.key` without `none`, and the three item descriptions (cpu, mem, count) with the exact texts and titles; `Header` derives `system?.stats?.claude_count ?? null`.
- §4.5: `aria-label="system usage"`; the two header sketches show `◔ cpu 12%   ◑ mem 41%` and, in cards, `… 2 procs`.
- §4.6: the mock returns `cpu_pct` 5–30, `mem_used_bytes ≈ 13 GiB ± 3 %`, `mem_total_bytes` 32 GiB, `claude_count` 2.
- §7: INFO `claude processes changed {count}`; DEBUG `system sample {elapsed_ms, count, cpu_pct, mem_used_bytes, did_prime}`.
- §8 `system.rs` bullet: `count_claude` and `cpu_share` tests, the smoke test's new assertions; TypeScript bullet: `memPct` on used/total, one test per row of the seven-row table, the texts.
- §10: replace `- Whole-machine CPU and memory figures (decided against 2026-09-17).` with `- Per-process CPU or memory figures (removed 2026-09-17; the count remains).`

- [ ] **Step 3: 2026-09-15 design doc**

- §6.2 "**Sampler.**" paragraph: after `publishes \`SystemStats\`` insert ` (whole-machine CPU busy share, memory used and total, and the Claude process count; per-process figures were dropped 2026-09-17)`.
- §7 row description (the line `sparkline (hourly max week-all pct, last 7 days; missing hours are path` and its continuation `breaks, never zeros)`) → `sparkline (15-minute max week-all pct, last 24 hours; empty buckets are skipped, never zeros; the line is continuous)`.
- §8 `get_history` row: `The sparkline asks for 7 d / 1 h week-all once per cycle` → `The sparkline asks for 24 h / 15 min week-all once per cycle`.

- [ ] **Step 4: Read back and commit**

Run: `git diff --stat docs/` and read each hunk once for typos and broken fences (`grep -c '^```' <file>` must be even for each file).

```bash
git add docs/
git commit -m "Docs: continuous charts, machine system figures, 24-hour sparkline"
```

---

### Task 8: Visual verification against the mock and one live sampler check

Run by the orchestrator (the Playwright MCP and the app process are session tools).

- [ ] **Step 1: All four gates on the branch**

Run, in order: `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`, `npm test`, `npm run build`. All green before anything else.

- [ ] **Step 2: Mock screenshots**

Start `VITE_MOCK_BACKEND=1 npm run dev` in the background (note the port Vite prints). With the Playwright MCP: at 980 × 700 screenshot the table (continuous row sparklines, column header "24 hours", system line "cpu N% · mem N%", count on the chip, a Week note "Nd Nh left"); click one row's sparkline and screenshot the open drawer at 7d (continuous line, dots, strip `peak · avg · collapse`, no "missing"); resize to 420 × 640 for cards (count `2 procs` in the line); load `?mockSystem=error` (line reads "system usage unavailable"). Save screenshots to the scratchpad and send them to the user with `SendUserFile`. Stop the dev server.

- [ ] **Step 3: Live sampler check through the log**

Stop the running release instance (single-instance lock), run `npm run tauri dev` for about 30 s, then read the app log (the `open_log_dir` target; DEBUG level must be on in settings, or read the INFO `claude processes changed {count}` line) and confirm `system sample` lines with `cpu_pct` between 0 and 100 that change between samples, `mem_used_bytes` below the machine's total, and a `count` matching the open Claude Code sessions. Stop the dev app; rebuild and relaunch the release exe:

```bash
npm run build && cargo build --release --manifest-path src-tauri/Cargo.toml --features tauri/custom-protocol
```

then start `src-tauri/target/release/<exe>`.

- [ ] **Step 4: PR and merge**

Load the `coderabbit-budget` skill first. Push the branch, open the PR with the screenshots and a summary of the four changes, wait for CI, address any CodeRabbit finding that is a real defect, then `gh pr merge <n> --squash --admin --delete-branch`. Finally remind the user of the two open live gate checks from PR #4.
