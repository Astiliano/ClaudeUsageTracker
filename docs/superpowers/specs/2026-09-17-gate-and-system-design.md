# Gate reconciliation, Claude process usage, stay-on-top button — design

Branch `gate-and-system`. Decisions confirmed with Josh 2026-09-17: reconcile
the gate on every trigger plus a presence wake; usage figures are for the
Claude processes only; new items sit on a second header line; the Settings
"Keep window on top" toggle is removed in favour of a header button.

## 1. Goal

1. The header chip reflects the process gate on the first cycle, after every
   Refresh, and within one sample interval of Claude Code starting.
2. The header shows what the Claude Code processes cost: their CPU share,
   their total resident memory, and how many there are.
3. "Stay on top" is one click away in every layout.

## 2. Facts this design relies on (verified 2026-09-17)

- `Machine::decide` (machine.rs) moves the gate only in rule 6, and only for
  `Trigger::Timer`; Startup, Manual and AccountChanged are decided with
  `claude_running = None` (driver.rs: the Startup call before the loop, the
  `notified_manual` / `notified_startup` arms, `handle_account_changed`).
  The Timer arm checks `is_busy()` before spending `process.claude_running`.
  Pinned by `manual_and_startup_ignore_the_gate`.
- `DriverStatus { gate, busy, stalled_at, backoff_until }` is written only by
  `publish_status`; `core_get_dashboard` copies it into `Dashboard`. The UI
  refetches the dashboard on `usage:updated`, `gate:changed` and
  `poller:stalled` (all three through one 250 ms debounce) and reloads
  dashboard plus history on `cycle:finished` (not debounced).
- `process::is_claude_running(&mut System, exclude_pid)` refreshes processes
  with `ProcessRefreshKind::nothing().with_exe(OnlyIfNotSet).with_cmd(OnlyIfNotSet)`,
  collects matched pids into a local `Vec`, logs them at DEBUG and returns
  `!matched.is_empty()`. `matches_claude(name, cmd)` is the pure matcher.
  `SysinfoProbe` wraps a `Mutex<System>` for the gate only.
- sysinfo 0.39: per-process `cpu_usage()` is diff-based and (on Windows)
  the first refresh after a process is discovered seeds no baseline, so a
  true value needs **three** refreshes with at least
  `MINIMUM_CPU_UPDATE_INTERVAL` (200 ms) between each (§4.1);
  `Process::cpu_usage()` is per-core (can exceed 100), `System::cpus().len()`
  normalises it to a machine share; `Process::memory()` is RSS in bytes;
  `System::total_memory()` needs `refresh_memory()`.
- Triggers reach the driver through one `tokio::sync::Notify` per kind
  (`Triggers` in triggers.rs); `Core` holds `triggers`, `status`, `binary`;
  `EventSink` (driver.rs) is implemented by `TauriEvents` (tray.rs) and by
  `SilentEvents` in the driver tests. The driver's `pid_slot: Arc<AtomicU32>`
  holds the live poll child's pid (0 when none).
- Header: `topbar-left` (title + count, hidden in cards) and `topbar-actions`
  (chip, refresh, settings); `chipFor(dashboard)` derives the chip from
  `bannerFor`. `Ring` is 44 px with the value inside and a label under it;
  `metricColor(pct)` gives the ok/warn/crit colour at 70/95.
  `Prefs.alwaysOnTop` is applied by an `App` effect through
  `backend().setAlwaysOnTop`; capability `core:window:allow-set-always-on-top`
  exists. `Settings` renders a `Toggle` "Keep window on top".
- The mock backend's `listen` is a no-op, so anything the mock UI must show
  changing over time has to be driven from the mock itself.

## 3. Change 1 — the gate reconciles on every decided trigger

### 3.1 `machine.rs`

`Trigger` gains `Presence` (wire form `presence`; it does **not** bypass
backoff). `SkipReason` gains `AlreadyActive` (wire form `already_active`).

`decide` rules 4 and 6 become:

4. Candidate list. `Timer` and `Presence` require `claude_running == Some(_)`
   (`None`: `debug_assert!`, then `Skip(GateIdle)`). `Timer`: Idle & !running
   → `Skip(GateIdle)`; otherwise all enabled. `Presence`: Active & anything →
   `Skip(AlreadyActive)`; Idle & !running → `Skip(GateIdle)`; Idle & running →
   all enabled. `Manual` / `Startup`: all enabled regardless of the answer.
   `AccountChanged(ids)`: `ids ∩ enabled`. Rule 5 (backoff) is unchanged.
6. A `Run` **with a process answer** reconciles the gate:
   - Idle & `Some(true)` → Active, `gate_transition = Some(Active)`, for
     every trigger (opening the gate early on a subset poll is harmless: the
     next Timer polls everyone).
   - Active & `Some(false)` → Idle, `gate_transition = Some(Idle)`, **only
     for `Timer`, `Manual` and `Startup`**, whose candidate list is the whole
     enabled set. That run is the final poll. **Invariant: the run that
     closes the gate polls every enabled account not in cooldown.** (A
     partially backed-off Timer already closes the gate without the
     cooling accounts today; that gap is pre-existing and unchanged.)
     `AccountChanged` polls `ids ∩ enabled` and therefore never closes the
     gate; with Claude gone it runs its subset and leaves the gate Active
     for the Timer's final poll.
   - `None` never moves the gate. A `Skip` never moves the gate.

Consequences: a Manual poll with Claude gone while Active is the final poll,
so the following Timer skips as `gate_idle` instead of polling once more; a
Startup or Manual poll with Claude running while Idle lands on Active and the
chip is right before the cycle even finishes (status is published right after
`decide`). `preview_manual` is unchanged: Manual still runs from either gate.

Full table (gate, answer → candidates, transition), Skips omitted:

| trigger | Idle, Some(true) | Idle, Some(false) | Active, Some(true) | Active, Some(false) | None |
|---|---|---|---|---|---|
| Timer | all, →Active | skip gate_idle | all, — | all, →Idle | skip gate_idle (debug_assert) |
| Presence | all, →Active | skip gate_idle | skip already_active | skip already_active | skip gate_idle (debug_assert) |
| Manual / Startup | all, →Active | all, — | all, — | all, →Idle | all, — |
| AccountChanged | ids∩enabled, →Active | ids∩enabled, — | ids∩enabled, — | ids∩enabled, — | ids∩enabled, — |

### 3.2 `driver.rs`

- New helper `async fn probe_if_free(&self) -> Option<bool>`: `None` when
  `self.shutdown.is_cancelled()` (DEBUG "process check skipped: shutting
  down", so no walk is spent during the exit window) or when
  `is_busy()`, otherwise `blocking(move || Ok(process.claude_running(pid))).await`
  with `process = Arc::clone(&self.process)` and `pid = self.current_pid()`
  (`blocking` takes an `FnOnce() -> AppResult<T>`). An `Err` from the hop
  (the blocking task was cancelled or panicked) is logged at WARN
  ("process check failed; gate left unchanged") and yields `None`: no
  answer never moves the gate, whereas a guessed `false` could close it.
  The walk (tens of ms) moves off the driver task through the existing
  `blocking` hop, the same one the store calls use, because `AccountChanged`
  fires on every add, enable, disable and rescan. The extra await is safe:
  this task is the only caller of `decide` and `begin_cycle` (the
  load-bearing invariant in `run()`), a chosen `select!` arm runs to
  completion before another is polled, and the only thing that can change
  busy during the await is a cycle *ending*, which makes the answer we are
  about to decide with strictly more current, never stale in the unsafe
  direction. Busy is still checked before a process check is spent, so the
  app's own poll child can never latch the gate; a busy decision then skips
  as `busy` exactly as today.
- The Startup call before the loop, the `notified_manual` and
  `notified_startup` arms and `handle_account_changed` pass
  `self.probe_if_free().await` instead of `None`. The Timer arm uses the
  same helper and keeps its explicit short-circuit: `None` (busy, or a
  failed check) → debug, move `last_cycle_end`, continue — the Timer arm
  never hands `None` to `decide`, so rule 4's `debug_assert!` stays
  unreachable.
- `SysinfoProbe` applies the shared exclusion from §4.1 (`Exclusion`):
  it caches its own pid and start time on first use and excludes the poll
  child by pid **and** any process whose parent is this app, so the gate
  and the sampler can never disagree about which processes count. Today the
  gate excludes only `exclude_pid`.
- New select arm `_ = self.core.triggers.notified_presence()`:
  - `lock_machine().gate() == Active` → `debug!` and continue (no probe
    spent; the machine's `already_active` rule is the backstop).
  - busy → `debug!`, set `presence_deferred = true`, continue. The wake is
    edge-triggered (§4.3) and the count stays `> 0` afterwards, so a wake
    consumed while a cycle runs would otherwise be lost until the next
    Timer. `presence_deferred` mirrors `changed_deferred`: at the three
    points where `flush_deferred_changes` runs (backstop reap, `done_rx`
    reap, watchdog abort) the driver also calls `self.core.triggers.presence()`
    and clears the flag, so the wake is re-decided the moment the cycle ends.
  - otherwise `match self.probe_if_free().await { None => debug!("presence
    skipped: process check failed") and continue, Some(r) =>
    decide_and_maybe_run(Trigger::Presence, Some(r), …) }`. Like the Timer
    arm, the presence arm never hands `None` to `decide`. (The busy case is
    caught by the explicit `is_busy()` check above, so `None` here can only
    be a failed hop.) A presence cycle is a normal cycle: `live = Some(cycle)`;
    when it finishes the deadline is `last_cycle_end + interval_secs` like
    any other.
  - Other skips of a presence decision (`gate_idle` because the fresh probe
    disagrees with the sampler, `all_backed_off`, `halted`, `no_binary`,
    `no_enabled_accounts`) are **not** deferred: none of them would poll
    anyway, and the next Timer reconciles the gate. Startup is never a
    problem: it probes for itself (§3.2), so with Claude open at launch the
    gate is Active before the first sample and the wake skips as
    `already_active`.
- `decide_and_maybe_run` logs `AlreadyActive` at DEBUG alongside `GateIdle`
  and `Busy`, and returns `None` without deciding (DEBUG `trigger ignored:
  shutting down`) once `self.shutdown.is_cancelled()`: the select picks
  randomly among ready arms, so without this a presence or manual wake that
  lands during the 2.5 s exit window could start a cycle on an app that is
  closing.

### 3.3 `triggers.rs`

`presence: Notify`, `pub fn presence(&self)`, `pub async fn notified_presence(&self)`.
The sampler (§4.3) is the only caller of `presence()`.

## 4. Changes 2 and 3 — Claude process usage and count

### 4.1 Data (`src-tauri/src/system.rs`, new module)

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClaudeStats {
    /// Claude Code processes other than the poll child and any child of this app.
    pub count: u32,
    /// Sum of their resident memory, bytes.
    pub rss_bytes: u64,
    /// Sum of their CPU usage as a share of the whole machine, 0..=100.
    /// `None` only when the CPU count is unknown (0); see §4.1 on sampling.
    pub cpu_pct: Option<f32>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemStats {
    pub sampled_at: i64,          // epoch ms
    pub mem_total_bytes: u64,     // denominator for the memory ring only
    pub claude: ClaudeStats,
}
pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
```

Pure, unit-tested pieces:

- In `process.rs`, shared by the gate and the sampler (the gate never
  depends on `system.rs`):
  `ProcView { pid: u32, parent: Option<u32>, start_time: u64, name: String, cmd: Vec<String>, rss_bytes: u64, cpu: f32 }`
  — the slice of `sysinfo::Process` both readers build (`start_time` is
  sysinfo's seconds-since-epoch process start), with
  `ProcView::from(&sysinfo::Process)`; and
  `Exclusion { self_pid: u32, self_started_at: u64, poll_child: Option<u32> }`
  with `fn counts(&self, view: &ProcView) -> bool` =
  `matches_claude(name, cmd) && pid != poll_child && !(parent == Some(self_pid) && start_time >= self_started_at)`.
  The parent clause removes the race where the child has been spawned but
  `pid_slot` is not yet written; the start-time clause stops a recycled pid
  from excluding a real session: Windows reports `th32ParentProcessID`
  even after the parent has died, so a `claude.exe` older than this app
  whose original parent's pid was later reused by this app is still counted.
  `is_claude_running` is rewritten on top of `Exclusion::counts` (its
  `exclude_pid` argument becomes the `Exclusion`), and its existing tests
  keep passing with `self_pid = 0, self_started_at = 0`.
- `aggregate(procs: &[ProcView], exclusion: &Exclusion, cpus: usize) -> ClaudeStats`:
  sums `rss_bytes` and `cpu` over `exclusion.counts(view)`. `cpu_pct` is
  `Some(sum(cpu) / cpus as f32)` clamped to 0..=100; `None` when `cpus == 0`
  (never expected; defensive).
- `presence_edge(prev_count: u32, next_count: u32) -> bool` = `prev == 0 && next > 0`.

`Sampler { system: System, self_pid: u32, self_started_at: u64, pid_slot: Arc<AtomicU32>, cpus: usize, mem_total_bytes: u64, primed: bool }`
with `sample(&mut self, now_ms) -> Sampled` where
`Sampled { stats: SystemStats, elapsed_ms: u64, did_prime: bool }` (`did_prime`
is true only on the call that ran the priming steps, unlike the sticky
`Sampler.primed` field; `elapsed_ms` is the
wall time of the whole call, so the log shows the priming cost once):

Let `PROCESS_REFRESH` = `refresh_processes_specifics(All, true, nothing().with_exe(OnlyIfNotSet).with_cmd(OnlyIfNotSet).with_cpu().with_memory())`.

- First call only (`!primed`), in this order:
  1. `refresh_cpu_specifics(CpuRefreshKind::nothing())` — initialises the
     CPU list without opening the PDH usage query, which `refresh_cpu_usage()`
     would do on every call; the per-process CPU share is divided by
     `cpus().len()` inside sysinfo, so **this must precede the first
     process refresh** or every `cpu_usage()` reads 0. Cache
     `cpus = cpus().len()`.
  2. `refresh_memory()` once; cache `total_memory()` (a constant).
  3. `PROCESS_REFRESH`, then `std::thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL)`.
  4. Read `self_started_at` from `system.process(self_pid).start_time()`
     (0 if absent, which disables the start-time clause and falls back to
     the plain parent check).
  5. `PROCESS_REFRESH` again, then sleep again. Set `primed`.

  Steps 3 and 5 are priming rounds: sysinfo's Windows `compute_cpu_usage`
  stamps `last_update` on every call but only seeds the `old_*` baseline
  once the interval has elapsed, so the first refresh seeds nothing, the
  second diffs against zero (a since-boot average) and seeds, and only the
  third — the common-path refresh below — is a true diff. About 400 ms
  once, on the blocking pool.
- Every call: `PROCESS_REFRESH`, build `ProcView`s,
  `aggregate(&views, &Exclusion { self_pid, self_started_at, poll_child: pid_slot (0 → None) }, cpus)`.

A Claude process that appears mid-run reads 0 CPU in the sample it first
appears in and a since-boot average in the next; a true share arrives on its
third sample (about 10 s), for the same seeding reason. Documented, accepted: the memory and count are
exact from the first sample and are what the line is for.

The sampler has its **own** `System`; the gate's `SysinfoProbe` is untouched,
so the two never contend and a gate refresh cannot disturb the CPU diff
baseline. Cost: one process walk per 5 s (tens of ms on a blocking thread;
the first call about 400 ms more for the two priming sleeps); the CLI is never
spawned for this.

### 4.2 Publishing

- `Core.system: Arc<Mutex<SystemSlot>>` with
  `SystemSlot { stats: Option<SystemStats>, stopped: bool }` (`Default`:
  `None`, `false`); `lock_system` helper next to `lock_status`,
  poison-tolerant like the others. `stopped` is set once by the sampler's
  terminal break (§4.3) so a slot that never received a sample is
  distinguishable from one that is still warming up.
  `Core` is built as an exhaustive literal in three places that all gain the
  field: `lib.rs` setup, the `core()` fixture in `commands.rs` tests and
  `test_core()` in `driver.rs` tests.
- `EventSink::system_sampled(&self)`; `TauriEvents` emits `system:sampled`
  with no payload; `SilentEvents` gets an empty impl.
- Command `get_system` → `SystemReport { stats: Option<SystemStats>, stopped: bool }`
  via `core_get_system(core)`: a clone of the slot, no store access.
  Registered in `generate_handler!`.

### 4.3 The sampler task (`system::run_sampler`)

Spawned from `lib.rs` setup next to the driver, with `Arc<Core>`,
`Arc<dyn EventSink>`, the shared `pid_slot` and the shutdown
`CancellationToken`. `pid_slot: Arc<AtomicU32>` is created in setup and
passed to both: `Driver::new(core, events, process, binary, shutdown, pid_slot)`
gains it as a sixth parameter (it no longer builds its own at the end of
`new`), and the driver test helper `test_driver()` passes a fresh
`Arc::new(AtomicU32::new(0))`. `run_sampler(core, events, pid_slot, shutdown)`
is the task entry point.

```
const MAX_CONSECUTIVE_PANICS: u32 = 3;
let mut sampler = Sampler::new(Arc::clone(&pid_slot));   // std::process::id() inside
let mut prev_count = 0;
let mut panics = 0;
let mut wait = Duration::ZERO;                            // first sample at once
loop {
    select! { _ = shutdown.cancelled() => break, _ = sleep(wait) => {} }
    let (next, sampled) = match spawn_blocking(move || { let s = sampler.sample(now()); (sampler, s) }).await {
        Ok(pair) => { panics = 0; pair }
        Err(join) => {
            panics += 1;
            error!(error = %join, attempt = panics, "system sample panicked");
            if panics >= MAX_CONSECUTIVE_PANICS {
                error!("system sampler stopped after repeated panics");
                lock_system(&core.system).stopped = true;   // stats, if any, are left for the stale rule (§4.4)
                events.system_sampled();
                break;
            }
            sampler = Sampler::new(Arc::clone(&pid_slot));
            wait = SAMPLE_INTERVAL;       // never a tight loop
            continue;
        }
    };
    sampler = next;
    let Sampled { stats, elapsed_ms, did_prime } = sampled;
    if shutdown.is_cancelled() { break; }   // cancellation landed during the sample: publish nothing, wake nobody
    if presence_edge(prev_count, stats.claude.count) { info!("presence wake"); core.triggers.presence(); }
    if prev_count != stats.claude.count { info!(count = stats.claude.count, rss_bytes = stats.claude.rss_bytes, "claude processes changed") }
    debug!(elapsed_ms, count = stats.claude.count, rss_bytes = stats.claude.rss_bytes, cpu_pct = ?stats.claude.cpu_pct, did_prime, "system sample");
    prev_count = stats.claude.count;
    lock_system(&core.system).stats = Some(stats);
    events.system_sampled();
    wait = SAMPLE_INTERVAL;
}
```

The first sample is taken at once (and is already a valid diff, §4.1), then
one every 5 s. A panicking sample (a `spawn_blocking` join error) is logged
at ERROR and the sampler is rebuilt after a normal interval; three in a row
stop the task for the rest of the run and set `stopped`, emitting once so
the UI refetches. Any stats already in the slot are left there: the UI's
stale rule (§4.4) dims them with their sample time; a slot with no stats
and `stopped` reads "system usage unavailable" rather than "waiting". A stopped sampler also stops presence
wakes, so until the next launch the gate reconciles only on Timer and
Manual (AccountChanged can open it, never close it) — the pre-change
latency for "Claude started while idle" — which the ERROR line records. The task
exits on shutdown and re-checks cancellation after the blocking sample so it
never fires a presence wake into a driver that is shutting down; it holds
nothing that needs flushing.

The presence wake costs at most one poll per Idle→Active transition (the gate
then stays Active until a Timer sees no Claude), never a poll for statistics.
The driver re-probes before deciding, so a stale or racy sample can only
cause a `gate_idle` skip, never an unwanted poll.

### 4.4 Frontend data

`types.ts`: `ClaudeStats`, `SystemStats` mirroring §4.1 (`cpu_pct: number | null`)
and `SystemReport { stats: SystemStats | null; stopped: boolean }` mirroring §4.2.

`src/hooks/useSystem.ts` → `{ report: SystemReport | null; error: string | null }`
(`report` is `null` until the first response):
loads `get_system` on mount, subscribes to `system:sampled` and reloads on
each; the same sequence guard as `useDashboard` (a late response never
overwrites a newer one). A successful response **always** replaces the
report, including one whose `stats` is `null`; only a rejected invoke keeps
the previous report and sets `error` (no toast: a failure every 5 s would be
a toast storm); the next success clears it. Subscription and pending work
are torn down on unmount. `App` owns the hook (as it owns `useDashboard`)
and passes `report`, `systemError` and the existing `now` tick to `Header`
as three new props (`system: SystemReport | null; systemError: string | null; now: number`).
`Header` renders `SystemLine` and derives the count for `chipFor` as
`system?.stats?.claude.count ?? null` (never a non-null assertion).

All rendering decisions are made by the pure `systemLine` (below), which
returns the items **and** whether the line is dimmed, so every state is
unit-tested; the component only maps the result to markup:

Rows 1–5 are exclusive and evaluated top to bottom (first match wins);
rows 6–8 are dim modifiers applied, in that order, only to a row 4 or 5
match, the first true one supplying the reason. `stats = report?.stats ?? null`,
`stopped = report?.stopped ?? false`:

| # | condition | items | dimmed | reason (appended to every item `title`) |
|---|---|---|---|---|
| 1 | `stats === null && error !== null` | "system usage unavailable" | no | the error |
| 2 | `stats === null && stopped` | "system usage unavailable" | no | "sampler stopped, see log" |
| 3 | `stats === null` (no report yet, or a report without stats) | "waiting for first sample" | no | — |
| 4 | `stats.claude.count === 0` | "no Claude processes" | rows 6–8 | rows 6–8 |
| 5 | `stats.claude.count > 0` | cpu, mem, [count when `showCount`] | rows 6–8 | rows 6–8 |
| 6 | (with 4 or 5) `error !== null` | as above | **yes** | the error |
| 7 | (with 4 or 5) `stopped` | as above | **yes** | "sampler stopped, see log" |
| 8 | (with 4 or 5) `isStale(stats, now)` (`now - sampled_at > 3 × 5 s`) | as above | **yes** | "last sample HH:MM:SS" |

An error outranks a null-stats report (row 1 before 3), and stopped stats
dim at once (row 7) rather than 15 s later when they turn stale.

`sampled_at` is therefore read, and a dead sampler cannot show a plausible
frozen figure for the rest of the run.

`src/lib/system.ts` (pure, tested):

```ts
export function formatBytes(bytes: number): string;
  // < 1 GiB → "840 MB" (MiB, no decimals); ≥ 1 GiB → "1.2 GB" (GiB, one decimal); 0 → "0 MB"
export function memPct(stats: SystemStats): number | null;
  // rss/total*100 clamped 0..100; null when total is 0
export const SAMPLE_INTERVAL_MS = 5000;
export const STALE_AFTER_MS = 3 * SAMPLE_INTERVAL_MS;
export function isStale(stats: SystemStats, now: number): boolean;   // now - sampled_at > STALE_AFTER_MS
export interface SysItem { key: "cpu" | "mem" | "count" | "none" | "waiting" | "unavailable"; pct: number | null; text: string; title: string }
export interface SysLine { items: SysItem[]; dimmed: boolean }
export function systemLine(input: { report: SystemReport | null; error: string | null; showCount: boolean; now: number }): SysLine;
  // states per the table above; the reason for a dim goes into every item's title, after the item's own title
  // waiting:     { key: "waiting",     pct: null, text: "waiting for first sample", title: "the first figures arrive within a second of launch" }
  // unavailable: { key: "unavailable", pct: null, text: "system usage unavailable", title: error ?? "sampler stopped, see log" }
  // count 0 → the single item { key: "none", pct: null, text: "no Claude processes", title: "no Claude Code process is running" }
  //           in every layout (no rings for zeros)
  // cpu:   pct = cpu_pct,      text "cpu 3%"  / "cpu —" when null, title "Claude processes: 3% of the machine's CPU"
  // mem:   pct = memPct,       text "mem 1.2 GB",             title "Claude processes: 1.2 GB of 32.0 GB (4%)" (both through `formatBytes`)
  // count: pct = null, only when showCount: text "2 procs" / "1 proc", title "Claude Code processes running"
export function processCountSuffix(count: number | null): string;
  // null or 0 → ""; 1 → " · 1 Claude process"; n → " · n Claude processes"
```

`present.ts`: `chipFor(dashboard, claudeProcesses: number | null = null)`
appends `processCountSuffix(claudeProcesses)` to the `active` and `idle`
texts only (halted/stalled/no-binary/no-accounts chips are unchanged), and
`countPlacement(kind: BannerKind, compact: boolean): "chip" | "line"` decides
where the count goes: `"chip"` only when `!compact` and the kind is `active`
or `idle`; `"line"` otherwise (cards, and every other chip in every layout,
so a halted or stalled header still says how many Claude processes exist).
`Header` computes `placement = countPlacement(banner?.kind ?? "idle", compact)`
from the `banner` it already derives (`bannerFor` is typed `Banner | null`)
and passes the count to `chipFor` when it is `"chip"` and `showCount` to the
system line when it is `"line"`. The count is therefore always shown exactly
once. An idle chip beside a non-zero count is a real, transient state
(the presence wake resolves it within one decision) or an honest one
(`all_backed_off`: Claude runs, every account is cooling, nothing polls);
the chip text is not changed for it.

### 4.5 Frontend rendering

Per the dataviz method: a ratio against a limit is a **meter**, so CPU share
and memory share are rings; the count is a plain figure, never a ring.

- `Ring` gains `size?: "md" | "sm"` (default `"md"`, existing callers
  unchanged). `gauge.ts`: `RING_SIZES = { md: RING, sm: { size: 20, stroke: 3, radius: 8.5 } }`
  (`RING` stays exported for `AccountCard`). Props become a discriminated
  union: `{ pct; title? } & ({ size?: "md"; label: string } | { size: "sm" })`,
  so `sm` takes no label. `sm` draws track and arc only (no centred text, no
  label element) inside `<span className="ring-sm" aria-hidden="true">`
  — not `.ring`, whose 64 px column layout is for the cards; the visible
  text beside it is the accessible name. Colours as today: arc
  `metricColor(pct)`, track `var(--track-off)`.
- `src/components/SystemLine.tsx`, props
  `{ system: SystemReport | null; error: string | null; showCount: boolean; now: number }`:
  calls `systemLine` and renders
  `<div className={"sysline" + (dimmed ? " sysline-stale" : "")} role="group" aria-label="Claude process usage">`
  with each `SysItem` as `<span className="sysline-item" title>`, a `sm`
  ring for `cpu` and `mem` only, and the text in `.sysline-text` (mono
  12 px, `#adb6bd`, the chip's text ink; text never wears the ring colour).
  No state logic lives in the component.
- `Header` renders `SystemLine` directly under `.topbar` in every layout,
  above the banner bar. CSS: `.sysline { display:flex; align-items:center;
  gap:14px; flex-wrap:wrap; }`, `.sysline-item { display:flex; align-items:center; gap:6px; }`,
  `.sysline-stale { opacity: 0.55; }`, `.ring-sm { display:inline-flex; flex:none; }`.
  `.topbar-actions` gains `flex-wrap: wrap` so at 360 px the three buttons
  wrap under the chip instead of overflowing. At 360 px the three line items
  wrap to two lines at most.

Header at full width:

```
Usage Tracker  3 accounts        ● polling every 300 s · 2 Claude processes  [refresh] [on top] [settings]
◔ cpu 3%   ◑ mem 1.2 GB
```

cards:

```
● idle · waits for Claude Code   [refresh] [on top] [settings]
◔ cpu 3%   ◑ mem 1.2 GB   2 procs
```

### 4.6 Mock backend

`get_system` returns `{ stats, stopped }` with `sampled_at: Date.now()` on
every call (so `isStale` never trips against the mock) and stats that drift
a little per call (count 2, rss ≈ 1.2 GB ± 5 %, cpu 1–8 %). `stopped` is
`false` unless the page URL carries `?mockSystem=stopped` (`createMockBackend`
reads `window.location.search` once at creation; the real backend never
looks at the URL), in which case the
mock returns `stopped: true` from the start (with the last stats, so the
dimmed presentation is visible); `?mockSystem=error` makes `get_system`
reject with `{ code: "internal", message: "mock: sampler unreachable" }` so
the "unavailable" item can be screenshotted. `listen("system:sampled", h)`
starts a 5 s `setInterval` calling `h` and returns an unlistener that
clears it; every other event stays a no-op.

## 5. Change 4 — "on top" in the header

- `Header` props gain `stayOnTop: boolean` and `onToggleStayOnTop: () => void`.
  Between refresh and settings: `<button type="button" className={"btn" + (stayOnTop ? " btn-edit-on" : "")} aria-pressed={stayOnTop} title="Stay on top of other windows">on top</button>`.
  Present in every layout (it lives in `topbar-actions`, which cards keep).
- `App` passes `prefs.alwaysOnTop` and `() => update({ alwaysOnTop: !prefs.alwaysOnTop })`.
  The existing effect keeps applying the pref through `backend().setAlwaysOnTop`
  and routing failure to `showError`; persistence stays in `parsePrefs`.
- `Settings` loses the "Keep window on top" `Toggle`; nothing else there
  changes. The capability entry stays.

## 6. Error handling

- Sampler (§4.3): a panicking sample is logged at ERROR and the `Sampler`
  is rebuilt after a normal interval; three consecutive panics stop the task
  for the run, leaving the last sample in the slot for the UI's stale rule
  to dim. `cpus == 0` or `total_memory() == 0` yield `None` / a 0
  denominator that the pure helpers turn into "—", never a division by zero.
- `probe_if_free` (§3.2): a failed blocking hop is a WARN and `None`; the
  Timer and Presence arms skip that decision, the other triggers decide
  without an answer and leave the gate unmoved.
- `useSystem` (§4.4): a rejected `get_system` keeps the last report and sets
  `error`; the line dims with the message as title, no toast. A stale
  `sampled_at` dims the line the same way; a stopped sampler with no stats
  reads "unavailable".
- Presence wake while busy, already Active, or after a failed check is a
  DEBUG line, not an error.
- Everything else keeps the existing paths: `poll_now` and `get_dashboard`
  are untouched; the always-on-top effect still routes to `showError`.

## 7. Logging

- INFO: `gate changed {gate, trigger}` (now fires for every trigger that
  moves it); `claude processes changed {count, rss_bytes}`; `presence wake`
  when the sampler fires the trigger.
- WARN: `process check failed; gate left unchanged {error}`.
- ERROR: `system sample panicked {error, attempt}`; `system sampler stopped
  after repeated panics`.
- DEBUG: `system sample {elapsed_ms, count, rss_bytes, cpu_pct, did_prime}`;
  `presence skipped {reason}` for busy / already_active / no process
  answer; `trigger ignored: shutting down {trigger}`; `process check
  skipped {reason}` with reasons `shutting down` and `busy` (only
  `probe_if_free` names why there is no answer; a failed hop stays the
  WARN above); the existing `decision skipped`
  line now also covers `already_active`.
- The existing `process gate check` DEBUG line is unchanged; which trigger
  spent it is one line above in `decision skipped` / `gate changed`.

## 8. Testing

Four gates on every task: `cargo test --manifest-path src-tauri/Cargo.toml`,
`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`,
`npm test`, `npm run build`. TDD throughout.

Rust:

- `machine.rs`: `a_startup_cycle_with_claude_running_lands_on_active`
  (Idle, `Startup`, `Some(true)` → Run, `gate_transition == Some(Active)`,
  `gate() == Active`); `a_manual_cycle_with_claude_gone_lands_on_idle`
  (Active, `Manual`, `Some(false)` → Run, transition `Some(Idle)`);
  `a_manual_cycle_with_claude_gone_while_idle_stays_idle`; the pinned test
  becomes `manual_and_startup_without_a_process_answer_keep_the_gate`
  (`None` from either gate: Run, no transition);
  `account_changed_opens_the_gate_but_never_closes_it` (Idle + `Some(true)`
  → transition Active; Active + `Some(false)` → Run of the subset, no
  transition, gate still Active) and
  `an_account_changed_that_finds_claude_gone_does_not_consume_the_final_poll`
  (after that run, a Timer with `Some(false)` still runs all enabled and
  closes the gate);
  `presence_runs_only_from_idle_with_claude_running` (the four combinations:
  (Idle,true) Run+Active, (Idle,false) `gate_idle`, (Active,true) and
  (Active,false) `already_active`, gate unmoved on every skip);
  `presence_respects_backoff` (Idle, running, every account cooling →
  `all_backed_off`, gate unmoved); the `None`-answer case is added to
  `a_timer_without_a_process_answer_skips_as_gate_idle` under its existing
  `cfg!(debug_assertions)` guard (the `debug_assert!` panics under
  `cargo test`);
  `wire_forms_are_snake_case` extended with `presence` and `already_active`
  (`presence_respects_backoff` already pins that a skipped Presence leaves
  the gate unmoved).
- `driver.rs`: `probe_if_free_spends_no_process_check_while_busy` (a
  counting `ProcessProbe`; with a `CycleToken` held the count stays 0 and the
  result is `None`; without, 1 and `Some`; `#[tokio::test]` since the probe
  goes through `blocking`); `a_trigger_after_shutdown_starts_no_cycle`
  (cancel the token, `decide_and_maybe_run(Manual, Some(true))` → `None`,
  status not busy); `startup_with_claude_running_publishes_active`
  (`decide_and_maybe_run(Startup, Some(true))` then the published status gate
  is Active); `a_presence_decision_is_skipped_at_debug_when_already_active`
  (the machine rule, through `decide_and_maybe_run`);
  `a_presence_wake_skipped_while_busy_is_refired_when_the_cycle_ends`
  (the `presence_deferred` flag, tested the same way the
  `changed_deferred` tests exercise their flag).
- `triggers.rs`: `presence_coalesces_like_manual` (two `presence()` calls,
  one `notified_presence()` wake).
- `process.rs`: `Exclusion::counts` — counts a `claude.exe` and an npm-form
  `node`; ignores the poll child by pid; ignores a child of `self_pid`
  started after the app even when `poll_child` is `None`; **counts** a
  process whose parent is `self_pid` but whose `start_time` predates
  `self_started_at` (recycled pid); `self_started_at == 0` falls back to the
  plain parent check. The existing `is_claude_running` matcher tests are
  kept and rewritten to build an `Exclusion`.
- `system.rs`: `aggregate` — sums `rss_bytes` over counted processes only;
  `cpu_pct` is `sum / cpus`; clamps at 100; `cpus == 0` → `None`; empty
  input → zeros. `presence_edge` at (0,0), (0,1), (1,2), (2,0).
  The sampler loop's panic handling is tested through a `SampleFn`-style
  seam only if the implementation splits the loop from the blocking call;
  otherwise the counter and the `wait` reset are covered by a pure
  `after_panic(panics) -> Option<Duration>` helper (None = stop) and its test.
  `Sampler::sample` is exercised once against the real `System` under
  `#[test]` to pin that it returns without panicking, that
  `stats.mem_total_bytes > 0`, that `stats.claude.cpu_pct.is_some()` and
  `did_prime` is true on the first call and false on the second (a smoke
  test, not a value assertion).
- `commands.rs`: `core_get_system_reports_no_stats_before_the_first_sample_then_the_sample_then_stopped`.

TypeScript (`system.test.ts`, `present.test.ts`, `gauge.test.ts`):

- `formatBytes`: 0, 512 MiB, 1 GiB, 1.25 GiB, 20 GiB; `memPct` clamps and
  handles a 0 total; `systemLine` — one test per row of the §4.4 state
  table (items, `dimmed`, the reason appended to titles); `count == 0`
  yields the single `none` item whatever `showCount` is; `isStale` at
  exactly 15 s (false) and 15 s + 1 ms (true); `systemLine` item order,
  texts and titles with and without `showCount`, singular "1 proc",
  "cpu —" when `cpu_pct` is null; `processCountSuffix` at null/0/1/2.
- `chipFor` with a count appends to active and idle only; with `null` the
  texts are unchanged; halted/stalled/no-binary/no-accounts never get a suffix.
  `countPlacement`: `"chip"` for active/idle when not compact; `"line"` for
  those two when compact and for the other four kinds in both modes.
- `RING_SIZES.sm` geometry: `ringDash` at 0/50/100 with radius 8.5.
- `parsePrefs` tests unchanged (`alwaysOnTop` already covered).

Visual (Playwright MCP against the mock, screenshots attached to the PR):
980 px full with the system line and the count on the chip; 420 px cards
with the count in the line; `?mockSystem=stopped` (dimmed line) and
`?mockSystem=error` ("system usage unavailable"); "on top" pressed and released, persisted across
a reload; Settings without the toggle. Then one `npm run tauri dev` run to
confirm: chip shows active on the first cycle with a Claude session open,
Refresh with Claude closed flips it to idle, opening Claude while idle flips
it back within ~5 s and a `presence` cycle appears in the log, and the
system line shows a plausible RSS for the open sessions.

## 9. Documentation

- `docs/2026-09-15-claude-usage-tracker-design.md`: §6.5 rules 4 and 6
  rewritten per §3.1 with a "2026-09-17" note, `Presence` and
  `already_active` added to the enum wire forms, the driver-loop sketch
  gains the presence arm; §6.2's match rule is rewritten around
  `Exclusion` (poll child by pid, plus children of this app started after
  it) and gains a "sampler" paragraph pointing at `system.rs`; §8 gains
  `get_system` and the `system:sampled` event.
- `docs/superpowers/specs/2026-09-16-history-and-compact-design.md` §5.5:
  one-line note that the toggle moved to the header on 2026-09-17.

## 10. Out of scope

- Whole-machine CPU and memory figures (decided against 2026-09-17).
- Per-process breakdown or a history of process usage.
- Pausing the sampler while the window is hidden.
- Sleep/wake handling for the sampler (a late sample is harmless).
- Tray tooltip changes.
