# Memory guard, idle weight, native density, scaling charts — design

Branch `optimize`. Decisions come from `.claude-work/optimize/BRIEF.md`, and
Josh delegated product calls to the session. `research-memory.md` and
`research-window.md` resolve both OPEN items in the brief. The deviations
are listed in §2 with their reasons.

## 1. Goal

Each item below is a check that passes or fails.

1. An automatic refresh never spawns `claude` while the machine's available
   commit is below `min_free_memory_mb`. The figure is read before every
   spawn, not once per cycle (§4.3a). The header chip's visible text carries
   the figure, and it changes while the window is open without a reload
   (§4.3 event). The first sampler tick that sees the floor regained starts
   the held refresh.
2. Each cycle logs the poll child's peak working set and peak commit at
   INFO.
3. With the window closed to the tray, no `msedgewebview2` process belongs
   to the app. A tray click or a second launch brings the window back at its
   last size, position and maximized state.
4. The sampler ticks every 30 s while no window exists and every 5 s while
   one does.
5. No web font ships. There is no centered column and no rounded card. The
   page gutter is 6 px and rows are 40 px. The three layouts switch at
   breakpoints computed from `SHELL_PADDING`, never typed by hand. Every
   CSS length a TS constant depends on is set from TS (§6), so the two
   cannot drift.
6. The history chart is legible at 573x240 (the smallest table window; cards
   have no drawers) and fills a 1600x900 window: opening it scrolls its row
   to the viewport top, and row plus drawer is the viewport height (±1 px)
   wherever `CHART_MAX_PX` does not bind (§7). The sparkline track grows with the
   window width. The md rings exist only in the cards layout (below 573
   local px), and they scale with the card width across that range, from
   360 px up. The sm ring is a fixed 20 px glyph.

## 2. Decisions and deviations

- D1 Figure: system available commit, `(CommitLimit − CommitTotal) ×
  PageSize`, read with `K32GetPerformanceInfo`. sysinfo 0.39.6 cannot
  supply it: `available_memory()` is physical only, and `free_swap()`
  understates commit whenever CommitTotal < PhysicalTotal and can underflow
  (research-memory §b).
- D2 Scope of the hold: `Timer` and `Presence`. The brief says "Timer
  only, exactly like the process gate", but that gate already covers both
  automatic triggers (machine.rs:314-337), and a presence-woken spawn is
  exactly the case the guard exists for. Startup, Manual and AccountChanged
  bypass it, as the brief says. A clarification, not a reversal.
- D3 Source of the peak memory figure (DEVIATION). The brief's sampler
  walk cannot measure a poll: it samples every 5 s (30 s hidden), reads
  instantaneous RSS, and excludes the poll child (process.rs:79). Instead
  `run_usage` reads the OS lifetime peak counters on the child's handle
  (`K32GetProcessMemoryInfo`): exact peaks, one syscall per second of poll.
- D4 `Machine::decide` takes a `Facts` struct (DEVIATION in shape only).
  Two more inputs make nine parameters including `self`, eight without;
  either trips clippy `too_many_arguments` (threshold 7), and an `#[allow]`
  would loosen a lint. All 40 `.decide(` sites change mechanically: 38
  tests in machine.rs, driver.rs:724 (production) and driver.rs:1588.
- D5 The font picker is removed (DEVIATION, a consequence of the brief).
  Once the three @fontsource packages are gone, "plex" and "jetbrains" would
  render as fallbacks. The system stack is therefore the only face, and
  `Prefs.font`, `FONTS` and the Typeface section go.
- D6 Tauri moves to 2.12 as task 1, before the destroy path ships (§5.1).
  wry 0.56.1 fixes the WebView2 teardown crash wry#1794 and only
  tauri-runtime-wry 2.12.1 can use it; 2.12.0 also stops destroyed
  webviews leaking listeners (#15604, #15617); tauri-plugin-window-state
  2.5 requires tauri ^2.12. Hypothesis, validate before building: the
  2.11.5 APIs this repo calls are unchanged in 2.12.
- D7 Window geometry is kept by `tauri-plugin-window-state` 2.5 with
  `StateFlags::SIZE | POSITION | MAXIMIZED`. `VISIBLE` is excluded: with it,
  the plugin calls `show()` itself.
- D8 Breakpoints are computed from `gridMinWidth(...) + SHELL_PADDING`.
  Hand-typed literals drift from the CSS.
- D9 TS owns every shared length. The shell sets CSS custom properties from
  the layout constants (§6), and styles.css only reads them. A
  css-contract test fails on a literal where a variable belongs.
- D10 The chart height is measured in JS, not derived in CSS (§7). A
  container-query or `vh` height cannot express "the viewport minus the
  drawer's own chrome", and neither can be unit-tested.

Rejected alternatives:

- Physical `available_memory()`: it is blind to the commit limit, which is
  what kills this machine.
- `free_swap()`: see D1.
- A guard outside the Machine, such as a pre-check in the driver: that is a
  second gate with its own state, which the brief forbids.
- A new `Trigger::MemoryRecovered`: it would behave exactly like Timer in
  every rule. The wake is distinguished in logs instead.
- Null for idle weight, keeping `hide()`: 6 WebView2 processes and 325 MB
  stay resident with no window, the measured cost the brief targets.
- WebView2 `additionalBrowserArgs` memory flags: they shave a fraction and
  keep the processes.
- A hand-rolled geometry store (~70 lines plus a clamp): the plugin
  handles a detached monitor at no cost. Rejected unless #3594 bites.
- Staying on tauri 2.11: wry#1794 stays exposed on every recreate.
- The null design for the guard (no guard): this machine runs about 12
  sessions against a ~70 GB commit limit, and each refresh adds 200-400 MB
  per account.
- Playwright visual tests: a new toolchain and dependency for four CSS
  rules. Named manual checks (§10) cover them instead.
- Chart sizing in CSS: `container-type: size` on the content-height
  `.drawer` collapses it; `clamp(72px, 38dvh, 420px)` cannot subtract the
  drawer chrome; a `100dvh` flex chain makes the page an app-shell
  scroller for every view, to size one element.
- Re-checking memory only once per cycle (null for F3): with N accounts the
  2nd..Nth spawns would ignore the floor, which breaks goal 1.
- Deferring rule 6's gate close to the end of a complete cycle (for the
  cut final poll, §4.3a): it adds a pending-close state to `Machine` and
  moves when every closing Run publishes its gate. Reopening the gate in
  `hold_mid_cycle` changes only the cut case.
- A CSS-literal parse test as the only guard (null for D9): it detects
  drift but keeps two owners; custom properties leave one.

## 3. Facts this design relies on (verified 2026-10-06)

- machine.rs: `decide(&mut self, trigger, claude_running: Option<bool>,
  binary_present, halted, enabled: &[String], now: i64)` (287-409). Rules:
  0 halted (297), 1 busy (301), 2/3 (305-310), 4 candidates (312-359;
  Timer/Presence need a process answer, `debug_assert` 315-337), 5 backoff
  (369-389; a skip never moves the gate, 381-383), 6 gate reconciliation
  (391-402). `now` is epoch ms (driver.rs:722). `Trigger` 31-40,
  `SkipReason` 66-76 / `as_str` 78-90, `DriverStatus` 123-142, `Machine
  { gate, cycle, backoff }` 149-153, `status()` 233-245.
- driver.rs: `EventSink` 28-36 (impls `TauriEvents` tray.rs:291-321,
  `SilentEvents` driver.rs:1031, `Recorder` driver_loop.rs:68); a skip
  emits no event (737-747). `ProcessProbe` 39-41. `Driver::new(core,
  events, process, binary, shutdown, pid_slot)` 443-461, called at
  lib.rs:234, driver_loop.rs:194 (`driver_for`) and seven driver.rs tests
  (1109-1312). `run_cycle` loops accounts serially (285), one `run_usage`
  each (305), logs "poll finished" (373-392) and "cycle finished"
  (398-399). `probe_if_free` 484-508 returns `None` while busy.
  `settings()` fallback literal 579-586. `decide_and_maybe_run` 707-761.
  Timer arm 843-862 sets `last_cycle_end` when nothing runs; settings arm
  863-878 calls `reset_all_backoff`.
- triggers.rs: one `Notify` per kind (9-16); copy `presence()` /
  `notified_presence()` (33-35, 70-72).
- system.rs: `SAMPLE_INTERVAL` = 5 s (15), `SystemStats` 20-32, `sample()`
  114-165, `refresh_memory()` 137. `run_sampler(core, events, pid_slot,
  shutdown)` 170-249 is `pub` (`pub mod system`, lib.rs:10): select
  182-185, presence wake 229-232, `wait = SAMPLE_INTERVAL` 247; no test
  calls it today. `after_panic(panics: u32) -> Option<Duration>` 82-88
  returns `Some(SAMPLE_INTERVAL)`; test 299-301.
- lib.rs: `show_main_window` 37-43 does nothing without a window; the
  single-instance callback calls it (53-55). Tray MENU_OPEN 178, MENU_QUIT
  `app.exit(0)` 203, left-click 206-215. CloseRequested hide handler
  252-266. `ExitRequested` 277-296 shuts down on every request and ignores
  `code`; `EXIT_APPROVED` (35) lets the self-issued second exit through
  (279, 292). tauri 2.11.5 creates config windows before `setup`
  (app.rs:2524-2525).
- tray.rs: `should_hide_on_close` 167-170 and its test 556-559;
  `TauriEvents` 291-321. `emit` with no webview is a silent no-op.
- commands.rs: `Core` 53-78 (`status: Arc<Mutex<DriverStatus>>`,
  `close_to_tray: AtomicBool`, `settings_tx: watch::Sender<UserSettings>`).
  `Dashboard` 94-102, built at 153. `core_set_settings` 354-380 sends only
  when `polling_relevant_changed` (367-368). `UserSettings` test literals:
  commands.rs:647, driver.rs:1068, driver_loop.rs:133. `Core` literals,
  each gaining the new fields: commands.rs:663, lib.rs:124,
  driver.rs:1091, driver_loop.rs:161 (moves to tests/common, §10).
- store/settings.rs: key/value rows (schema.rs:27-30), so a new key needs
  no migration; `get_u32(key, default)` 85-90. `UserSettings` 18-26,
  `validate_settings` 29-49, `polling_relevant_changed` 56-60.
- usage/runner.rs: `RunResult { outcome, raw, duration_ms }` 138-143.
  `run_usage` races `timeout(timeout, child.wait())` against
  `cancel.cancelled()` (267-270). Its unit tests are pure; process tests
  live in tests/runner_guard.rs on the real `fake_claude` (`run_with` 58,
  `a_timeout_kills_the_child_and_records_the_limit` 178). tokio 1.53.1
  `Child::raw_handle()` returns `None` once reaped (process/mod.rs:1229-1232).
- windows-sys 0.61.2 is in Cargo.lock (4994-4997), not in Cargo.toml. Its
  ProcessStatus module has `K32GetPerformanceInfo` (mod.rs:44),
  `K32GetProcessMemoryInfo` (47), `PERFORMANCE_INFORMATION` (85-100) and
  `PROCESS_MEMORY_COUNTERS` (103-114; `PeakWorkingSetSize`,
  `PeakPagefileUsage`).
- tauri.conf.json: one window, label "main", 980x640, minimum 360x240, no
  `visible` key.
- Fonts and prefs: main.tsx:1-7 imports seven @fontsource CSS files
  (package.json:14-16); theme.ts:26-49 holds `FontKey`, `FONTS`,
  `isFontKey`; `parsePrefs` builds from `fresh()`, so it ignores unknown
  keys (prefs.ts:40-65); App.tsx:55-60 sets `--ui`/`--mono` inline;
  Settings.tsx:133-152 is the Typeface section, and the numeric-field
  pattern (draft, commit on blur or Enter, rollback) is at 66-79, 213-246.
- Layout: layout.ts `BREAKPOINTS = {narrow: 820, cards: 640}`,
  `SHELL_PADDING = 95` (2×22 + 2×1 + 2×16 + 17), pinned by layout.test.ts
  (7, 32). columns.ts `GRID_GAP = 10`, `gridMinWidth` 105-113 (takes each
  track's first px value), the `spark` track `"76px"`; it imports only
  `./reorder`, so layout.ts can import it without a cycle.
  AccountsTable.tsx:27 `DEFAULT_ROW_H = 66` mirrors `.row-grid` (drag
  fallback, 178). useViewport.ts tracks `innerWidth` with a
  `ResizeObserver`.
- styles.css: `.app` padding `28px 22px 48px`, `.app-inner` `max-width:
  1120px` (40-41); `.thead` `gap: 10px; padding: 11px 16px` (103-105);
  `.row-grid` `gap: 10px; padding: 14px 16px; height: 66px` (142); `.spark`
  32 px with a 24 px SVG (182-185); `.ring` `width: 64px` (204),
  `.ring-label` `max-width: 64px`, `.ring-sm` an unsized inline-flex span
  (67); `.drawer` an auto-height flex column, padding `16px 18px 18px 56px`
  (213-218); `.chart` `height: 148px; padding: 14px 16px` (225-228), labels
  10 px (245-248). Radii on `.panel` 101, `.card` 199-202, `.chart`,
  `.banner` 92-96, `.modal-body`.
- vitest runs `environment: "node"` over `src/**/*.test.ts`, so a test can
  `readFileSync("src/styles.css")`; no component or hook test exists.
- useDashboard.ts refetches only on `usage:updated`, `gate:changed`,
  `poller:stalled` and `cycle:finished` (121-125).
- Charts: Ring.tsx:28-31 sets fixed `width`/`height` = `geometry.size`
  plus a viewBox; Sparkline viewBox `0 0 100 24`,
  `preserveAspectRatio="none"`; HistoryDrawer.tsx:175-196 viewBox
  `0 0 100 100`, dots placed by percentage.
- Status: banner.ts `bannerFor` precedence halted > stalled > no_binary >
  no_accounts > active > idle; present.ts `chipFor(dashboard,
  claudeProcesses)` 56, `countPlacement` 82; `formatBytes` in
  src/lib/system.ts:13; mockBackend.ts `isUserSettings` 48-57, range check
  444-450, no test file.

## 4. Change 1 — memory guard

### 4.1 `src-tauri/src/memory.rs` (new)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ChildPeak { pub working_set_bytes: u64, pub commit_bytes: u64 }

pub const MIB: u64 = 1_048_576;
pub fn floor_bytes(min_free_memory_mb: u32) -> u64;  // u64::from(mb) * MIB
pub fn commit_headroom(limit_pages: u64, total_pages: u64, page_size: u64) -> u64; // saturating
pub fn available_commit_bytes() -> Option<u64>;     // cfg(windows): K32GetPerformanceInfo; else None
pub fn merge_peak(a: Option<ChildPeak>, b: Option<ChildPeak>) -> Option<ChildPeak>; // field-wise max
pub fn recovery_due(held: bool, available: Option<u64>, floor_bytes: u64) -> bool; // held && avail >= floor
pub enum ReadingLog { Lost, Restored }
pub fn reading_log(was_lost: bool, reading: Option<u64>) -> Option<ReadingLog>; // transitions only (§4.3)
#[cfg(windows)]
pub fn process_peak(handle: std::os::windows::io::RawHandle) -> Option<ChildPeak>; // K32GetProcessMemoryInfo

/// The one seam both readers use: the driver's guard (§4.3) and the
/// sampler's recovery wake (§4.4). Tests inject `FakeMemory`.
pub trait MemoryProbe: Send + Sync { fn available_commit_bytes(&self) -> Option<u64>; }
pub struct RealMemoryProbe;   // delegates to available_commit_bytes()

/// Every probe read in the driver and in the cycle task (§4.3, §4.3a).
/// `lost.swap(reading.is_none())` yields `was_lost`, so two readers on
/// different tasks log each transition once between them.
pub fn read_memory(probe: &dyn MemoryProbe, lost: &AtomicBool) -> Option<u64>; // logs per reading_log
/// What an automatic cycle carries into its task (§4.3a).
#[derive(Clone)]
pub struct MemoryGuard { pub probe: Arc<dyn MemoryProbe>, pub floor_bytes: u64, pub lost: Arc<AtomicBool> }
```

- Cargo gets `[target.'cfg(windows)'.dependencies] windows-sys = { version =
  "0.61", features = ["Win32_System_ProcessStatus", "Win32_Foundation"] }`
  (`K32GetProcessMemoryInfo` takes a `Foundation::HANDLE`). No new crate is
  downloaded.
- Every `unsafe` block carries a SAFETY comment: the out-param is zeroed
  plain data and `cb` is set to its size. BOOL 0 returns `None`.
- `available_commit_bytes()` is one syscall and takes no lock, so the driver
  calls it inline.
  - Hypothesis, validate before building: the call costs under 1 ms. The
    smoke test logs the elapsed time. If it is higher, the read moves into
    the `probe_if_free` blocking hop.

### 4.2 `machine.rs`

```rust
pub struct Facts<'a> {
    pub claude_running: Option<bool>,
    pub available_commit_bytes: Option<u64>,
    pub memory_floor_bytes: u64,
    pub binary_present: bool,
    pub halted: bool,
    pub enabled: &'a [String],
    pub now: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct MemoryHold { pub available_bytes: u64, pub floor_bytes: u64, pub since: i64 }

impl Trigger { pub fn is_automatic(&self) -> bool }  // Timer | Presence
SkipReason::LowMemory                                 // as_str "low_memory"
Machine { gate, cycle, backoff, hold: Option<MemoryHold> }
pub fn decide(&mut self, trigger: Trigger, facts: &Facts<'_>) -> Decision;
pub fn memory_hold(&self) -> Option<MemoryHold>;
DriverStatus { gate, busy, stalled_at, backoff_until, memory_hold: Option<MemoryHold> }
pub enum HoldChange { Unchanged, Started, Refreshed, Released }
pub fn hold_change(before: Option<MemoryHold>, after: Option<MemoryHold>) -> HoldChange;
// None->None Unchanged; None->Some Started; Some->None Released;
// Some(b)->Some(a): Refreshed iff a != b, else Unchanged.
pub fn hold_mid_cycle(&mut self, available: u64, floor: u64, now: i64,
                      cycle_closed_gate: bool) -> Option<Gate>; // §4.3a; Some(Active) when it reopened
```

Rule 5b runs after rule 5 (backoff) and before rule 6:

- It applies when `trigger.is_automatic()`,
  `facts.available_commit_bytes == Some(a)` and
  `a < facts.memory_floor_bytes`.
- The result is `Skip(LowMemory)`, and `hold` becomes
  `Some(MemoryHold { available_bytes: a, floor_bytes, since })`. `since` is
  kept from an existing hold, otherwise it is `now`.
- Because the rule comes after rule 5, it holds only a decision that would
  otherwise Run.
- Because it returns before rule 6, it never moves the gate. A final poll
  that closes the gate is held, not lost: the gate stays Active and the next
  automatic decision retries it. The same holds when the cut comes
  mid-cycle, after rule 6 already closed the gate: `hold_mid_cycle` reopens
  it (below).
- A `None` reading fails open, and the driver logs it (§4.3). A floor of 0
  never holds.

Hold lifetime:

- Any `Run` clears `hold`, whatever the trigger. Manual still refreshes
  while held.
- A `Timer` decision that ends in any skip other than `LowMemory` clears it
  too, early return or not (`Halted`, `Busy`, `NoBinary`,
  `NoEnabledAccounts`, `GateIdle`, `AllBackedOff`). That decision is the
  current verdict, and memory no longer explains the skip. `decide` is a
  thin wrapper: it calls the rule body, then applies this lifetime rule to
  the result in one place, so no early return can miss it.
- A `Presence` skip other than `LowMemory` leaves `hold` untouched, because
  `AlreadyActive` says nothing about memory.
- `status()` copies `hold`. `preview_manual` is unchanged.
- `hold_mid_cycle` sets `hold` with the same `since` rule and never
  touches backoff. Only the cycle task calls it (§4.3a), alongside
  `record`. Gate: when `cycle_closed_gate` is true (the cut cycle's
  decision had `gate_transition == Some(Idle)`), it sets `gate = Active`
  and returns `Some(Active)`; otherwise it leaves the gate and returns
  `None`. This restores the pre-decision state exactly: while the cycle
  runs every `decide` returns `Busy` at rule 1, and rule 6 (machine.rs:398)
  is the only gate write, so nothing else moved the gate. The gate ends where rule 5b would have left it had
  the low figure been read at decide time: Active, final poll pending.

### 4.3 `driver.rs`, `triggers.rs`, `lib.rs`, `tray.rs`

- `Driver::new` gains a 7th parameter, `memory: Arc<dyn MemoryProbe>`, after
  `process`. lib.rs:234-241 builds one `Arc::new(RealMemoryProbe)` and
  passes the same `Arc` to the driver and to `run_sampler` (§4.4).
- `decide_and_maybe_run`:
  - reads the probe only when `trigger.is_automatic()`, and uses `None`
    otherwise;
  - builds `Facts` with `memory_floor_bytes =
    floor_bytes(settings.min_free_memory_mb)`;
  - takes `before = machine.memory_hold()`, decides, and reads `after` under
    the same lock;
  - passes both to `hold_change`, which selects the log line (§9) and the
    event below; the event is emitted after `self.publish()` (734), so the
    refetch sees the hold.
- `LowMemory` joins the DEBUG skip arm (737-747). The INFO line comes from
  `HoldChange::Started`.
- Reading-loss logging: every probe read goes through one free function,
  `memory::read_memory(probe, lost)` (§4.1). The Driver owns
  `memory_reading_lost: Arc<AtomicBool>`, created in `Driver::new` (no new
  parameter), and clones the same `Arc` into each `MemoryGuard` (§4.3a),
  so the decide path and the cycle task share one flag. It logs
  per `reading_log` (§4.1): `Lost` is WARN "memory reading unavailable; not
  holding", `Restored` is INFO "memory reading restored". Only actual reads
  call it (automatic decides and the §4.3a check); the `None` that bypass
  triggers put in `Facts` is never a read. A steady `None` logs once.

Event (the chip must change while the window is open):

- `EventSink` gains `fn memory_hold_changed(&self)`. `TauriEvents` emits
  `"memory:hold"` with no payload; `SilentEvents` and `Recorder` implement
  it (`Recorder` counts calls).
- It fires on `HoldChange::{Started, Refreshed, Released}`, from
  `decide_and_maybe_run` and from the mid-cycle hold (§4.3a). `Unchanged`
  emits nothing.
- `EventSink` also gains `fn settings_applied(&self)`, fired at the end of
  the settings arm after `publish()` (§4.3b). `TauriEvents` emits
  `"settings:applied"`: a backoff reset is otherwise invisible to the UI
  until the next event (Settings.tsx:70 refetches nothing). `Recorder`
  counts it, which is the driver_loop barrier for a settings change.
- Every emitted name lives in `src/lib/events.ts`: `REFETCH_EVENTS =
  ["usage:updated", "gate:changed", "poller:stalled", "memory:hold",
  "settings:applied"] as const`, `HISTORY_EVENTS = ["cycle:finished"]`,
  `SYSTEM_EVENTS = ["system:sampled"]`. useDashboard.ts:121-125 iterates
  the first two, useSystem.ts the third.
- tray.rs gains one `pub const` per event name (`EVT_USAGE_UPDATED`,
  `EVT_CYCLE_FINISHED`, `EVT_GATE_CHANGED`, `EVT_POLLER_STALLED`,
  `EVT_SYSTEM_SAMPLED`, `EVT_MEMORY_HOLD`, `EVT_SETTINGS_APPLIED`) and
  `pub const FRONTEND_EVENT_NAMES: [&str; 7]` built from those consts.
  Every `TauriEvents` emit call (today string literals at tray.rs:293-319)
  passes a const, never a literal, so the array test also covers the emit
  sites, which cannot be unit-tested without an `AppHandle`. A tray.rs test reads `../../src/lib/events.ts` (relative
  to tray.rs) with `include_str!` and asserts each name appears there.
- `cycle_finished` gains a parameter: `fn cycle_finished(&self, peak:
  Option<ChildPeak>)`. `TauriEvents` still emits `()`; the parameter makes
  the cycle's peak fold observable to `Recorder` (§4.5).

### 4.3a Re-check before every spawn (`driver.rs` `run_cycle`)

- `CycleInputs` gains `memory_guard: Option<MemoryGuard>` (§4.1: probe,
  floor bytes and the Driver's shared `lost` flag). `run_cycle`
  (driver.rs:264) is a free fn spawned at driver.rs:558 with no `&Driver`,
  so the guard is how it reads. `start_cycle` sets it only when
  `trigger.is_automatic()`; Startup, Manual and AccountChanged get `None`,
  matching the bypass ruling. It also gains `closed_gate: bool`, set by
  `decide_and_maybe_run` from `gate_transition == Some(Gate::Idle)`.
- Before every account after the first (the first was just checked by
  `decide`), the loop calls `read_memory(&*guard.probe, &guard.lost)`. If
  the figure is
  `Some(a)` with `a < floor`:
  - `reopened = lock_machine(&machine).hold_mid_cycle(a, floor, now,
    closed_gate)`, then `publish_status`;
  - `events.memory_hold_changed()`; if `reopened` is `Some(g)`, INFO "gate
    changed {gate: active, trigger: memory_hold}" and
    `events.gate_changed(g.as_str())`;
  - INFO "memory hold" with `mid_cycle = true, polled, remaining`;
  - break out of the account loop. The cycle ends normally
    (`cycle_finished`, token drop). No backoff is recorded for the
    unpolled accounts. A cut final poll leaves the gate Active (§4.2), so
    the next automatic decision, a Timer or the recovery wake, retries it
    instead of skipping with `GateIdle`. The UI sees idle then active.
- The held refresh, when released, is a full Run: accounts already polled
  in the cut cycle are polled again. That costs at most one extra poll
  each, and keeps "a Run polls every enabled account" intact.
- A `None` reading mid-cycle continues (fails open), as in `decide`.

### 4.3b Wake and settings arms (`driver.rs`, `triggers.rs`)

- `Triggers` gains `memory_recovered()` and `notified_memory_recovered()`, a
  `Notify` like `presence`.
- `async fn handle_timer(&self, settings, done_tx) -> Option<LiveCycle>` is
  the body of the Timer arm (843-862), moved unchanged. The Timer arm
  becomes `match self.handle_timer(..).await { Some(c) => live = Some(c),
  None => last_cycle_end = now_ms() }`.
- A new select arm handles `notified_memory_recovered()`:
  - if `memory_hold()` is `None`: DEBUG "memory wake ignored: no hold";
  - otherwise `handle_timer`, on a fresh figure (a flicker above the floor
    holds again). No busy branch: rule 1 precludes a decide-time hold while
    busy, and between a mid-cycle hold and the reap `probe_if_free` returns
    `None`, so the arm skips and the level-triggered wake (§4.4) re-fires.
- Settings arm (863-878): it keeps the previous `UserSettings`, and calls
  `reset_all_backoff` only when `polling_relevant_changed(&prev, &next)`.
  A floor-only change leaves backoff intact. When `memory_hold()` is
  `Some` after any change, it calls `triggers.memory_recovered()`, so a
  lowered floor applies without waiting for a tick. It ends with
  `publish()` then `events.settings_applied()`.

### 4.4 Sampler recovery (`system.rs`)

- `run_sampler` gains a parameter, `memory: Arc<dyn MemoryProbe>` (after
  `events`), the same `Arc` the driver holds. `Sampled` is unchanged; the
  figure never goes on the wire.
- A pure step decides each tick:

  ```rust
  pub struct SamplerStep { pub wake: bool, pub wait: Duration }
  pub fn sampler_step(held: bool, available: Option<u64>, floor_bytes: u64,
                      window_open: bool) -> SamplerStep;
  // wake = recovery_due(held, available, floor_bytes); wait = sample_interval(window_open)
  ```
- After each publish, `run_sampler` reads `held =
  lock_status(&core.status).memory_hold.is_some()`, `floor =
  floor_bytes(core.settings_tx.borrow().min_free_memory_mb)`,
  `memory.available_commit_bytes()` and `core.window_open`, and calls
  `sampler_step`. On `wake` it logs DEBUG "memory recovered; waking driver
  {available_bytes, floor_bytes}" and calls `triggers.memory_recovered()`.
  `wait` replaces `wait = SAMPLE_INTERVAL` at 247.
- The wake is level-triggered: while a hold exists, every tick above the
  floor wakes the driver. There is no edge flag to lose, and the cost is at
  most one decision per tick.

### 4.5 Peak child memory (`usage/runner.rs`, `driver.rs`)

- `RunResult` gains `peak: Option<ChildPeak>`.
- `run_usage` replaces the select at 267-270 with a loop whose deadline is
  fixed once, so the 1 s tick can never restart the timeout:

  ```rust
  let sleep = tokio::time::sleep_until(Instant::now() + timeout);
  tokio::pin!(sleep);
  let mut tick = tokio::time::interval(Duration::from_secs(1)); // first tick is immediate: the read just after spawn
  let waited = loop { tokio::select! {
      r = child.wait() => break Some(Ok(r)),
      _ = &mut sleep => break Some(Err(())),        // the existing Timeout path
      _ = cancel.cancelled() => break None,
      _ = tick.tick() => peak = merge_peak(peak, child.raw_handle().and_then(process_peak)),
  }};
  ```
  `Child::wait` is documented cancel safe, so re-polling it each iteration
  loses nothing. Flags and pid-slot handling are unchanged. The existing
  `a_timeout_kills_the_child_and_records_the_limit` (runner_guard.rs:178)
  is the regression test for the shape.
- "poll finished" gains `peak_working_set_bytes` and `peak_commit_bytes`;
  both fields are omitted when the peak is `None`. `run_cycle` folds the
  polls with `merge_peak`, "cycle finished" carries the cycle maximum
  (omitted when `None`), and `events.cycle_finished(peak)` receives it.
- Hypothesis, validate before building (M6): missing the last second
  before exit (the handle is gone once reaped) and not counting
  `claude.exe`'s own children leave the figure usable.

### 4.6 Setting

- New key `min_free_memory_mb`: `DEFAULT_MIN_FREE_MEMORY_MB = 1536`,
  `MAX_MIN_FREE_MEMORY_MB = 65536`, range `0..=65536`; added to
  `UserSettings`, `stored_settings`, `save_settings` and
  `validate_settings` (`out_of_range` above 65536).
- `polling_relevant_changed` keeps its D16 meaning (interval, timeout,
  binary: reset backoff). A new `driver_relevant_changed(prev, next) =
  polling_relevant_changed(..) || prev.min_free_memory_mb !=
  next.min_free_memory_mb` replaces it at commands.rs:367 as the send
  condition; the driver decides the backoff reset itself (§4.3b).
- Frontend: `types.ts` adds `UserSettings.min_free_memory_mb: number` and
  `Dashboard.memory_hold: {available_bytes; floor_bytes; since} | null`
  (numbers). mockBackend's `isUserSettings` (48-57) requires the field,
  its defaults gain 1536, and its range check (444-450) rejects values
  outside 0..=65536 with `out_of_range`.
- Settings.tsx gets a "Memory floor" field next to Poll gap: `memoryDraft`
  on the existing draft/commit/rollback pattern, `type=number min=0
  max=65536`, hint "MB free commit · 0 = never hold".

### 4.7 Chip

- `BannerKind` gains `"held"`, tone info (no banner bar), precedence
  `halted > stalled > no_binary > no_accounts > held > active > idle`. Its text,
  also the chip's title, is `Holding refresh: ${formatBytes(available)}
  free, floor ${formatBytes(floor)}`.
- `chipFor` returns `{dot: "warn", text: \`held · ${formatBytes(available)}
  free\`}`, so the figure is visible without hovering.
- `countPlacement("held", _)` returns `"line"`.
- The chip refreshes on `memory:hold` (§4.3 event): start, figure change
  and release each re-read `get_dashboard`.

## 5. Change 2 — window lifecycle, sampler cadence, fonts

### 5.1 Version bump (task 1)

- `tauri = { version = "2.12", features = ["tray-icon"] }`.
- `@tauri-apps/api` and `@tauri-apps/cli` move to `^2.12`.
- Regenerate Cargo.lock and package-lock.
- This is its own task, run through all four gates and the tray manual check
  (M1), so that any regression can be traced to the bump.

### 5.2 Close and reopen (`lib.rs`, `tray.rs`)

```rust
// tray.rs, next to the other pure helpers
pub enum ExitAction { Allow, KeepRunning, Shutdown }
pub fn exit_action(code: Option<i32>, close_to_tray: bool, approved: bool) -> ExitAction;
// approved -> Allow; code None && close_to_tray -> KeepRunning; otherwise Shutdown
// `approved` is EXIT_APPROVED.load(SeqCst) (lib.rs:35)

// commands.rs, next to Core
pub struct CreateGuard(Arc<Core>);                  // Drop: creating.store(false)
pub fn begin_create(core: &Arc<Core>) -> Option<CreateGuard>; // None if creating.swap(true) was true
pub fn window_created(core: &Core);   // window_open = true, sampler_kick.notify_one()
pub fn window_destroyed(core: &Core); // window_open = false, sampler_kick.notify_one()
```

Close and exit:

- Delete `should_hide_on_close`, its test, and the CloseRequested handler
  (252-266). A user close then proceeds, destroying the window and its
  WebView2.
- `ExitRequested { code, api }` matches on `exit_action`:
  - `KeepRunning`: `api.prevent_exit()` and INFO "window closed to tray".
  - `Shutdown`: the existing sequence (277-296).
  - `Allow`: return.
- Quit (`app.exit(0)`, which arrives as `Some(0)`) still shuts down. Close
  with close_to_tray off still quits.
- Core gains `window_open: AtomicBool` (seeded true), `creating:
  AtomicBool` and `sampler_kick: Notify`.
- `on_window_event` handles `WindowEvent::Destroyed` for "main": it calls
  `window_destroyed(&core)` and logs INFO "window destroyed".

`show_main_window(app)`:

- If the window exists: unminimize, show, focus.
- Otherwise, if `begin_create(&core)` returns a guard, move it into a
  build started with `tauri::async_runtime::spawn`. Building on the main
  thread (tray, menu, single-instance) deadlocks.
- The build is `WebviewWindowBuilder::from_config(&app,
  &app.config().app.windows[0])?.visible(false).build()`.
  - On success: show, focus, `window_created(&core)`, INFO "window created
    {elapsed_ms, x, y, width, height, maximized}" (outer position and size
    after the plugin's restore, so M1 can compare reopens).
  - On error: ERROR "window create failed {error}".
  - The guard drops at the end of the task, so `creating` resets on every
    path, panics included.
- When `begin_create` returns `None` (a second click or launch while a
  build runs), DEBUG "window create already running".
- tauri.conf.json's window gains `"backgroundColor": "#0b0d0e"` (the
  `--bg` value), so a recreated WebView2 does not paint white before the
  CSS loads. Hypothesis, validate before building: the 2.12 window config
  accepts `backgroundColor` and WebView2 honours it; M3 checks it.

Window state, first paint, frontend:

- `.plugin(tauri_plugin_window_state::Builder::default()
  .with_state_flags(SIZE | POSITION | MAXIMIZED).build())`. It restores in
  `on_window_ready` for every window, recreated ones included; its
  in-memory cache survives the destroy; it writes to disk on
  `RunEvent::Exit`, which the shutdown path reaches through `handle.exit(0)`.
- tauri.conf.json gains `"visible": false`, and `setup` ends with
  `show_main_window`, so the first paint lands at the restored geometry.
  Hypothesis, validate before building: the restore runs before that show.
- Frontend: no change. A recreated webview remounts, reloads and
  re-listens (useDashboard.ts:93-139, useSystem.ts:44-71); prefs persist in
  localStorage in the same WebView2 data folder.

### 5.3 Sampler cadence (`system.rs`)

- New items replace `SAMPLE_INTERVAL`:
  - `pub const VISIBLE_INTERVAL: Duration` = 5 s;
  - `pub const HIDDEN_INTERVAL: Duration` = 30 s;
  - `pub fn sample_interval(window_open: bool) -> Duration`.
- After a good sample the wait comes from `sampler_step` (§4.4).
- `after_panic(panics: u32, window_open: bool) -> Option<Duration>` returns
  `Some(sample_interval(window_open))` for the first two panics and `None`
  on the third; `run_sampler` passes `core.window_open.load(SeqCst)`. The
  test at system.rs:299 becomes
  `after_panic_waits_one_interval_for_the_window_state_then_gives_up_on_the_third`.
- Its select gains `core.sampler_kick.notified()`, which samples at once. A
  newly created window therefore gets a fresh figure before
  `STALE_AFTER_MS` (src/lib/system.ts:8) can trip.
- While hidden, presence and memory-recovery latency rise to at most 30 s.
  The brief accepts this trade.

### 5.4 Fonts

- Delete the three @fontsource dependencies and main.tsx:1-7; `FontKey`,
  `FONTS`, `FONT_KEYS`, `isFontKey` from theme.ts; `Prefs.font` and its
  parse branch; the Typeface section; the `--ui`/`--mono` entries of
  App.tsx's inline style (§6's `shellStyle(zoom)` replaces it).
- styles.css sets `--ui: "Segoe UI Variable Text", "Segoe UI", system-ui,
  sans-serif` and `--mono: ui-monospace, "Cascadia Mono", Consolas,
  monospace`.
- A stored `font` key is ignored by `parsePrefs`, so no migration is needed.

## 6. Change 3 — layout and density

The constants in src/lib/layout.ts and src/lib/columns.ts are the contract.
TS owns them, and the CSS reads them through custom properties (D9).

- Homes: `APP_GUTTER = 6`, `ROW_HEIGHT = 40`, `ROW_PAD_X = 8`,
  `PANEL_BORDER = 1`, `RING_MIN_PX = 36` and `RING_MAX_PX = 120` live in
  layout.ts (chart.ts imports layout.ts, never the reverse); `GRID_GAP`
  (now 8) stays in columns.ts. layout.ts's `./columns` import becomes a
  value import, and `NARROW_HIDDEN` moves above `BREAKPOINTS` (reading a
  `const` before its declaration at module load is a TDZ error).
- layout.ts exports `shellVars(): Record<string, string>`, returning
  `--gutter: 6px`, `--row-h: 40px`, `--row-pad-x: 8px`, `--grid-gap: 8px`,
  `--panel-border: 1px`, `--ring-min` and `--ring-max` (from `RING_MIN_PX`
  and `RING_MAX_PX` above; §7 uses them), and
  `shellStyle(zoom): CSSProperties`, which returns `{zoom, ...shellVars()}`
  (the one typed cast at App.tsx:56-61 moves with it). App.tsx uses
  `shellStyle(zoom)` in place of its inline object, so the spread is
  tested in layout.test, not only seen in M7.
- styles.css uses only the variables for those lengths:
  - `.app { padding: var(--gutter) }`; `.app-inner` loses `max-width` and
    centering, and its gap is `var(--gutter)`;
  - `.row-grid { height: var(--row-h); padding: 0 var(--row-pad-x); gap:
    var(--grid-gap) }`;
  - `.thead { gap: var(--grid-gap); padding: 6px var(--row-pad-x) }`, so the
    header tracks line up with the row tracks;
  - `.panel { border: var(--panel-border) solid var(--line) }`.
- Panels, cards, the chart, the banner and the modal get `border-radius: 0`
  and 1 px `var(--line*)` borders. The chip keeps its pill shape, because it
  is a status token, not a card.
- The 6/8 scale for the remaining paddings: `.card` `8px`, `.drawer`
  padding `6px 8px 8px 8px` (the 56 px left indent goes) and gap 6,
  `.chart` `6px 8px`, `.chart-dots` inset `6px 8px` to match.
- `AccountsTable` drops `DEFAULT_ROW_H` and imports `ROW_HEIGHT`.
- `SHELL_PADDING = 2*APP_GUTTER + 2*PANEL_BORDER + 2*ROW_PAD_X + 17`,
  which is 47.
- `BREAKPOINTS` replace the hand literals 820/640: `narrow =
  gridMinWidth(DEFAULT_ORDER) + SHELL_PADDING` = 654 + 7×8 + 47 = 757;
  `cards = gridMinWidth(visibleColumns(DEFAULT_ORDER, NARROW_HIDDEN)) +
  SHELL_PADDING` = 486 + 5×8 + 47 = 573.
- The `spark` track becomes `"minmax(76px,1fr)"`. `gridMinWidth` reads its
  first px value, 76, so both breakpoints are unchanged, and the track
  grows with the window like the meter columns.
- Header, system line and drawer arrangement and the palette are
  unchanged; only padding drops to the 6/8 scale.

## 7. Change 4 — charts scale with their container

History chart, sized in JS (D10):

- A new `src/lib/chart.ts`:

  ```ts
  export const CHART_MIN_PX = 72;    // local px
  export const CHART_MAX_PX = 1200;  // binds only on very tall screens
  export interface ChartFit { viewportPx: number; chromePx: number; zoom: number }
  /** Local px height that makes row + drawer equal the viewport, clamped. */
  export function chartHeightPx(f: ChartFit): number;
  // clamp(CHART_MIN_PX,
  //       Math.floor(f.viewportPx / f.zoom - (ROW_HEIGHT + 1) - f.chromePx / f.zoom),
  //       CHART_MAX_PX)      // + 1 is the row's bottom border
  ```
  `viewportPx` and `chromePx` are viewport (post-zoom) px; the row is
  `ROW_HEIGHT` local px from layout.ts, not measured (the drawer sits in
  its own `role="row"` wrapper, AccountRow.tsx:103-106, a sibling of the
  row grid at :85; both sit in the outer `.row` div at :84, whose 1 px
  `border-bottom` is the + 1); the result is local px for the inline
  `height`.
- A new `src/hooks/useChartHeight.ts(drawerRef, chartRef, rowRef, zoom)`.
  `zoom` travels as a prop: AccountsTable (already has it, :19) ->
  AccountRow -> HistoryDrawer. `rowRef` is created in AccountRow, attached
  to the outer `.row` div (:84), and passed to HistoryDrawer as a prop
  beside `zoom`. Rejected: `drawer.closest(".row")`, which ties the hook to
  a class name and silently scrolls nothing if the class changes.
  - `chromePx = drawer.getBoundingClientRect().height −
    chart.getBoundingClientRect().height`. That difference is the drawer's
    padding, head, controls, axis and gaps, and does not depend on the
    chart's own height, so the measure is not circular.
  - `viewportPx` is `window.innerHeight`.
  - It measures in `useLayoutEffect` and writes `chart.style.height`
    directly (no React state), so no frame paints the unsized chart. It
    recomputes on a `ResizeObserver` on the drawer and on `resize`, and
    disconnects both on unmount. The CSS `.chart` loses `height: 148px`.
  - After its first write it calls `scrollIntoView({block: "start"})` on
    `rowRef.current`, the outer `.row` (row grid, drawer and bottom
    border), so the row top meets the viewport top and the drawer ends at
    the fold instead of below it (the rows above it would otherwise push
    its x-axis off-screen). Scrolling the drawer's own wrapper instead
    would put the row grid, with the name and the toggle just clicked,
    41 px above the viewport. The page is the scroller (no `overflow` on `.app`), and
    row + drawer = viewport leaves enough document below to scroll that
    far. Resizes recompute the height but never scroll.
  - Hypothesis, validate before building: in this WebView2 (Chromium with
    standardized CSS `zoom`), `getBoundingClientRect` and `innerHeight`
    return post-zoom px while an inline `height` is local px. The column
    drag already assumes the first half (`toLocal`, AccountsTable.tsx:196).
    M7 measures at the Text size prefs "small" (0.92) and "largest" (1.22).
- Arithmetic at 1600x900, zoom 1. The drawer is head, controls, chart,
  axis (HistoryDrawer.tsx:144-196) with a 6 px flex gap. Chrome = padding
  6+8, head ~14, controls ~27, axis ~12, three gaps 18, top border 1 = ~86.
  Chart = 900 − 41 − 86 = 773 px, below `CHART_MAX_PX`, so row + drawer =
  900: it fills. The approximate terms are measured, not assumed; only the
  sum's independence from the chart height matters.
- At 573x240, zoom 1 (below 573 the cards layout has no drawers): 240 −
  41 − 86 = 113 px of chart. At 700x240, zoom 1.22 (the first table width,
  573 × 1.22 = 699.1): 196.7 −
  41 − 86 = 69.7, so the floor binds. The plot is the chart minus `.chart`
  padding 2×6 and border 2×1, so at the 72 px floor it is 58 px against 10
  px labels. Legible.
- HistoryDrawer: the viewBox and the percentage-placed dots already scale
  and are unchanged.

Rings:

- The SVG drops `width` and `height`, fills its wrapper (`width: 100%;
  height: auto; aspect-ratio: 1`) and keeps `viewBox 0 0 size size`, so the
  stroke scales with it. `ringViewBox(geometry): string` moves to gauge.ts.
- md (cards layout only): `.card-rings` is `container-type: inline-size`
  (its width comes from the card, so inline-size containment is sound).
  `.ring`'s fixed `width: 64px` becomes `width: clamp(var(--ring-min),
  28cqi, var(--ring-max))`, and `.ring-label` `max-width` becomes `100%`.
  `RING_MIN_PX` and `RING_MAX_PX` (layout.ts, §6) reach the CSS through
  `shellVars`. Three rings at 28cqi plus two 14 px gaps fit one
  line from a 360 px window up.
- sm: `.ring-sm { width: 20px; height: 20px }`, so the 100% SVG has a
  definite box. It is a glyph beside text and does not scale.

Sparkline:

- The `spark` track is `minmax(76px,1fr)` (§6), so the cell grows with the
  window. `.spark` keeps `height: 32px` inside the 40 px row and gains
  `width: 100%`; the SVG stays `width: 100%; height: 24px` with
  `preserveAspectRatio="none"`. Height is fixed by the row by design.

## 8. Error handling

- Memory probe `None`: the decision fails open, logged on transitions only
  (§4.3). A failed `process_peak` leaves `peak` as it was; a `None` cycle
  peak omits the fields from "cycle finished".
- Window build error: ERROR; `creating` resets, `window_open` stays false,
  the app runs windowless and the next tray click retries.
- Window-state plugin issues #3594 (deadlock) and #3474 (save lost when
  killed before Exit). Hypothesis, validate before building: nobody has
  diffed 2.5.0 for fixes. If #3594 reproduces in M1, the fallback (the
  hand-rolled store, §2) goes to Josh, not into a patch.
- Settings out of range: the existing `out_of_range` rejection and rollback.

## 9. Logging

- INFO: `memory hold {available_bytes, floor_bytes, trigger}` (Started);
  `memory hold {available_bytes, floor_bytes, mid_cycle, polled,
  remaining}` and, for a cut final poll, the existing `gate changed` with
  `trigger = memory_hold` (§4.3a); `memory hold released {available_bytes,
  floor_bytes, held_ms, trigger}`; `memory reading restored`; the existing
  `settings applied to the driver` gains `min_free_memory_mb,
  backoff_reset`; `poll finished` and `cycle finished` gain the peak
  fields; `window closed to tray`; `window destroyed`; `window created
  {elapsed_ms, x, y, width, height, maximized}`.
- WARN `memory reading unavailable; not holding` (transition only); ERROR
  `window create failed {error}`.
- DEBUG: `memory hold {available_bytes, floor_bytes}` (Refreshed); `memory
  recovered; waking driver`; `memory wake ignored: no hold`; `window create
  already running`; `decision skipped {reason: low_memory}`; `sample
  interval {secs}` when it changes.

## 10. Testing

Four gates run on every task:

- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
- `npm test`
- `npm run build`

TDD throughout.

### Rust

memory.rs:

- `commit_headroom_multiplies_free_pages_by_page_size`
- `commit_headroom_saturates_when_total_exceeds_limit`
- `floor_bytes_is_mebibytes`
- `merge_peak_takes_each_field_maximum`
- `merge_peak_keeps_some_over_none`
- `recovery_due_only_while_held_and_at_or_above_floor` (`None` gives false)
- `reading_log_fires_only_on_transitions` (four cases of was_lost × Some/None)
- `read_memory_sets_and_clears_the_shared_flag`: a local scripted probe
  `[None, None, Some(1)]` and one `Arc<AtomicBool>`; the flag reads true,
  true, false after the three calls, and the returned values equal the
  script (which transition logs is `reading_log`'s test)
- `#[cfg(windows)] available_commit_bytes_reads_a_positive_figure`, which
  logs the elapsed time
- `#[cfg(windows)] process_peak_of_this_process_is_positive`
- `#[cfg(not(windows))] available_commit_bytes_is_none_off_windows`

machine.rs:

- `a_timer_below_the_floor_is_held_and_records_the_hold`
- `a_presence_below_the_floor_is_held`
- `manual_startup_and_account_changed_ignore_the_floor`
- `a_hold_never_moves_the_gate`: Active, Timer, running false, low memory,
  and the gate stays Active
- `the_hold_comes_after_backoff`: all accounts backed off gives
  AllBackedOff, not LowMemory
- `an_unknown_reading_fails_open`
- `a_zero_floor_never_holds`
- `a_repeat_hold_keeps_since`
- `any_run_clears_the_hold`
- `every_non_low_memory_timer_skip_clears_the_hold`: table-driven over
  `Halted`, `Busy`, `NoBinary`, `NoEnabledAccounts`, `GateIdle`,
  `AllBackedOff`, each set up from a held machine
- `presence_skips_keep_the_hold`: `AlreadyActive`, `Halted`, `NoBinary`
- `hold_change_classifies_each_transition`: the five cases in §4.2,
  including `Some(b) -> Some(b)` as `Unchanged`
- `hold_mid_cycle_sets_the_hold_and_keeps_since`
- `hold_mid_cycle_reopens_a_gate_its_cycle_closed`: Active, Timer Run with
  running false (gate Idle), then `hold_mid_cycle(.., true)` returns
  `Some(Active)` and `status().gate` is Active; with `false` from an
  opening Run it returns `None` and the gate stays Active
- `status_carries_the_hold`
- The 38 existing machine.rs call sites and driver.rs:1588 move to a
  `facts(..)` test helper with floor 0 and must pass unchanged.

Other Rust tests:

- store/settings.rs: the defaults test gains 1536; the round trip includes
  the new key; 0 and 65536 are accepted, 65537 rejected;
  - `only_the_three_polling_keys_reset_backoff` (the existing test, kept:
    the floor is not a polling key);
  - `the_memory_floor_reaches_the_driver_but_is_not_polling_relevant`
    (`driver_relevant_changed` true, `polling_relevant_changed` false).
- tray.rs:
  - an eight-row table test for `exit_action` over (None, Some(0)) ×
    close_to_tray × approved;
  - `every_frontend_event_name_is_listed_in_events_ts` (§4.3).
- commands.rs:
  - `begin_create_is_single_flight_and_resets_on_drop`: a second
    `begin_create` is `None` while the first guard lives, `Some` after it
    drops;
  - `window_destroyed_clears_open_and_kicks_the_sampler` and
    `window_created_sets_open_and_kicks_the_sampler`: the flag flips, and
    `sampler_kick.notified()` completes within 100 ms (`notify_one` stores
    a permit).
- system.rs:
  - `sample_interval_is_5s_open_and_30s_hidden`;
  - `sampler_step_wakes_only_while_held_and_at_or_above_the_floor`
    (table: held × {None, below, equal, above});
  - `sampler_step_waits_by_window_state`;
  - `after_panic_waits_one_interval_for_the_window_state_then_gives_up_on_the_third`.
- tests/runner_guard.rs (the layer with the real `fake_claude` process):
  - `#[cfg(windows)] a_finished_run_reports_a_peak`: `run_with("slow",
    &[("FAKE_CLAUDE_SLEEP_SECS", "2")], 10 s, ..)` (fake_claude.rs:79), so
    the 1 s ticks read a live child (the default `emit` mode exits before
    the first read can be relied on); outcome ok, `peak` is `Some`, both
    fields `> 0`;
  - `#[cfg(not(windows))] a_finished_run_reports_no_peak`;
  - the existing `a_timeout_kills_the_child_and_records_the_limit` must
    still pass after the select becomes a loop (§4.5).

### tests/sampler_loop.rs (new)

Shared seam, `tests/common/mod.rs` (new): `defaults()`, `FakeMemory` (a
scripted `Mutex<VecDeque<Option<u64>>>` that pops one reading per call and
repeats the last; its one constructor, `FakeMemory::new()`, reads
`Some(u64::MAX)` until scripted, and `script(&[..])` replaces the queue and
is the only way either crate sets a reading), and `test_core(dir)
-> (Arc<Core>, watch::Receiver<UserSettings>)`, which becomes the only
`Core` literal under tests/ (driver_loop.rs:161 moves here). Each test
keeps the receiver alive: tokio's `watch::Sender::send` stores nothing
when no receiver exists (commands.rs:368 warns on exactly that), and
`run_sampler` reads the floor from the watch with no driver subscribed.
Both crates use every item (`defaults()` through `test_core`; `new` and
`script` for every reading, fixed figures included), so no `dead_code`
allow is needed. A common item one crate does not use is a clippy
`-D warnings` failure, fixed by using or removing it, never by an allow.

sampler_loop.rs drives the real `run_sampler` (real `Sampler`, which only
reads) with a sink counting `system_sampled`. Negative checks use a barrier,
not a sleep: twice, `sampler_kick.notify_one()` and wait for the count to
grow (the second sample proves the first one's step finished); then assert
`notified_memory_recovered()` does not complete within 100 ms
(`notify_one` stores a permit, so an earlier wake would be seen). Every wait is bounded at 10 s.

- `a_held_tick_above_the_floor_wakes_the_driver`: status carries a hold,
  `FakeMemory` above the floor; `notified_memory_recovered()` completes.
- `a_tick_below_the_floor_does_not_wake`: held, figure below; barrier, no
  wake.
- `a_kick_samples_at_once_while_hidden`: `window_open = false`; after the
  first sample, `sampler_kick.notify_one()`; the second sample lands within
  3 s (the hidden wait is 30 s).
- `the_floor_is_read_from_the_settings_watch`: hold set, figure 2 GiB.
  Order is the contract: `settings_tx.send` floor 4096 MB (receiver from
  `test_core` alive) BEFORE `run_sampler` is spawned, because the first
  sample is taken at once (`wait = Duration::ZERO`, system.rs:179) and a
  wake at the default 1536 MB floor would store a permit no later
  assertion can remove. Then spawn, barrier, no wake; then send 1024 MB,
  kick, wake.

### driver_loop.rs

- `harness()` builds its `Core` with `test_core` and keeps the receiver
  (`Harness._settings_rx`). `Harness` gains `memory: Arc<FakeMemory>`;
  `driver_for` passes it, and `harness()` builds it with `FakeMemory::new()`
  (`Some(u64::MAX)`), so no driver_loop test depends on this machine's free
  commit. The same holds for the driver.rs unit tests: tests/common is not
  reachable from `#[cfg(test)]` code in src, so driver.rs's test module
  defines a local `AmpleMemory` probe (`Some(u64::MAX)`), and all seven
  `Driver::new` sites (driver.rs:1109, 1123, 1147, 1168, 1219, 1264, 1312)
  pass it. Three of them drive automatic triggers (Timer at 1199 and 1323,
  Presence at 1208); with `RealMemoryProbe` and the 1536 MB default floor a
  low-commit moment would turn them into holds. `Recorder`
  gains `memory_holds` and `settings_applied` (`AtomicUsize`) and `peaks:
  Mutex<Vec<Option<ChildPeak>>>`.
- Settings changes go through `core_set_settings(&h.core, &s)` (already
  imported, line 18), never `settings_tx` directly, so the
  `driver_relevant_changed` send at commands.rs:367 is under test. The
  barrier for "the driver applied it" is `settings_applied` growing by one.
- Setup for a hold, so no test waits on the 10 s minimum interval:
  `harness(false)`, one account, let the Startup cycle finish with ample
  memory (`cycles == 1`; the gate stays Idle because the process answer
  was `false`), then `h.process.running.store(true)`, script the memory
  low, and fire `h.core.triggers.presence()`. Presence with gate Idle and
  `running = true` reaches rule 5b.
- Tests:
  - `a_presence_below_the_floor_spawns_nothing_and_publishes_the_hold`:
    no new `usage_updated`, `memory_hold` is `Some`, `memory_holds == 1`,
    gate still Idle.
  - `a_memory_wake_after_recovery_runs_the_held_refresh`: from the held
    state, script ample memory, call `triggers.memory_recovered()`; a cycle
    runs, the hold clears, `memory_holds == 2` (Released).
  - `a_memory_wake_without_a_hold_is_ignored`: no cycle within 1 s.
  - `lowering_the_floor_in_settings_releases_the_hold`: from the held
    state, `core_set_settings` with floor 0 (the figure stays low); a
    cycle runs and the hold clears.
  - `a_floor_only_change_keeps_backoff`: the Startup poll fails
    (`FAKE_CLAUDE_MODE=exit-nonzero`, as in
    `the_driver_publishes_gate_busy_and_backoff_into_shared_state`, 638),
    so `backoff_until` holds the account; `core_set_settings` with a
    floor-only change; once `settings_applied == 1`, `backoff_until` still
    holds the same entry. Control: an interval change, then
    `settings_applied == 2` and the entry is gone.
  - In the hold tests, `core_get_dashboard(&h.core)` carries the same
    `memory_hold` as the status (the commands.rs:153 copy).
  - `manual_refresh_runs_while_held_and_clears_the_hold`.
  - `a_held_final_poll_keeps_the_gate_active_until_it_runs`: Active gate,
    `running = false`, memory low, Presence is `AlreadyActive`, so this one
    uses a real Timer wait (one of two 10 s tests).
  - `a_cut_closing_cycle_keeps_the_gate_active` (the other; pins §4.2's
    reopen): `harness(true)`, two accounts; Startup opens the gate and
    polls both. Then `running = false`, script `[ample, low]`, wait for the
    Timer cycle: one new `usage_updated`, hold set, `gates` ends
    `["active", "idle", "active"]`, status gate Active. Then script ample
    and call `memory_recovered()`: both accounts poll, `gates` ends with
    `"idle"`, the hold clears.
  - `a_cycle_stops_before_the_next_spawn_when_memory_drops`: the
    Startup-then-Presence setup above (so not a 10 s test) with two
    accounts; after the Startup cycle, `running = true`, script
    `[ample (decide), low (before account 2)]`, fire
    `triggers.presence()`; after the cycle
    ends: one new `usage_updated`, `memory_hold.available_bytes` equals the
    low figure, `memory_holds == 1`, the gate is Active (opened by the
    Run), and `backoff_until` has no entry for account 2.
  - `a_manual_cycle_ignores_a_mid_cycle_drop`: same script, Manual
    trigger; both accounts poll.
  - `a_cycle_reports_a_peak` (`cfg(windows)`):
    `FAKE_CLAUDE_MODE=slow`, `FAKE_CLAUDE_SLEEP_SECS=2` (inside the 5 s
    timeout of `defaults()`); `peaks` holds one `Some` for the Startup
    cycle. This proves the wiring only: one account, and the env is
    process-wide, so per-poll peaks cannot be made to differ and no
    driver_loop test can tell a max fold from last-wins. The fold is
    `run_cycle` calling `merge_peak` once per poll, and its semantics are
    owned by the `merge_peak_*` unit tests in memory.rs.

### vitest (src/lib)

- banner.test: `held` sits between no_accounts and active, and its text
  names both figures.
- present.test: `chipFor` for held has the formatted available figure in
  its visible `text`; `countPlacement("held")`. Every `Dashboard` and
  `UserSettings` literal in banner.test, present.test and mockBackend.ts
  gains the new required field, or `npm run build` fails.
- events.test (new): the three arrays equal the §4.3 lists exactly.
- layout.test:
  - `SHELL_PADDING === 47`;
  - `BREAKPOINTS` equals `{narrow: 757, cards: 573}` (literal expected
    values; the old "equals the gridMinWidth sums" restated the code and
    goes);
  - `ROW_HEIGHT === 40`;
  - the `layoutFor` edges at 756/757 and 572/573, and at zoom 1.22;
  - `shellVars()` returns the px strings of `APP_GUTTER`, `ROW_HEIGHT`,
    `ROW_PAD_X`, `GRID_GAP`, `PANEL_BORDER`, `RING_MIN_PX`, `RING_MAX_PX`;
  - `shellStyle(1.22)` has `zoom: 1.22` and every `shellVars()` entry.
- css-contract.test (new; reads `src/styles.css` with `readFileSync`,
  extracts each named rule's declarations with a small regex):
  - `.app` padding is `var(--gutter)`; `.row-grid` height, padding and gap
    use `var(--row-h)`, `var(--row-pad-x)`, `var(--grid-gap)`; `.thead`
    gap and horizontal padding use `var(--grid-gap)` and
    `var(--row-pad-x)`; `.panel` border uses `var(--panel-border)`; `.ring`
    width uses `var(--ring-min)` and `var(--ring-max)`;
  - `.panel`, `.card`, `.chart`, `.banner`, `.modal-body` have
    `border-radius: 0`;
  - `.chart` declares no `height` (JS owns it);
  - every `var(--x)` it asserts is a key of `shellVars()`.
- columns.test: `GRID_GAP === 8`; `COLUMNS.spark.width` is
  `"minmax(76px,1fr)"`.
- prefs.test: a stored `font: "plex"` parses to prefs without `font`.
- theme.test: the font-key cases are dropped.
- gauge.test: `ringViewBox(RING_SIZES.md) === "0 0 44 44"`.
- chart.test: `chartHeightPx` at 900/zoom 1/chrome 86 is 773; at
  240/1/86 is 113; at 240/1/200 is `CHART_MIN_PX`; at 3000/1/86 is
  `CHART_MAX_PX`; at 900/1.22/105 is
  `Math.floor(900/1.22 − 41 − 105/1.22)`.
- mockBackend.test (new; `createMockBackend()` (mockBackend.ts:277), then
  `invoke("set_settings", …)` and `invoke("get_settings")`):
  `min_free_memory_mb` round-trips, 0 and 65536 are accepted, 65537 is
  rejected with `out_of_range`, and a payload without the field is
  rejected.

### Manual checks

Named in the plan; results are recorded in the work folder.

Each check passes only if every listed observation holds.

- M1 `tray-reopen`, on 2.12. Move and resize the window, then close to
  the tray and reopen 20 times. Pass: no crash; every reopen's INFO
  "window created" logs the same position and size (±2 px for DPI
  rounding); maximize, close and
  reopen: it opens maximized. Repeat once after quit and relaunch. Then,
  with DEBUG on and the window closed, click the tray icon twice within
  300 ms, and click it while launching a second instance: exactly one
  window, one "window created", and at least one "window create already
  running". Also click the tray immediately after a close: one window.
- M2 `idle-weight`: count the `msedgewebview2` processes whose parent chain
  reaches the app, before and after closing to the tray. Pass: 0 within
  10 s of the close.
- M3 `reopen-latency`: screen-record a tray click. Pass: "window created"
  logs `elapsed_ms` under 1500; the recording shows dashboard content
  within 2 s of the click; no white frame appears.
- M4 `cadence`: with DEBUG on. Pass: "system sample" lines are 30 s apart
  (±1 s) while closed to the tray, 5 s apart (±1 s) while open, and one
  lands within 1 s of the "window created" line.
- M5 `hold`: with the window open and a claude process running (gate
  Active; with the gate Idle a Timer skips as `GateIdle` before rule 5b),
  set the floor above the current available commit. Pass: within one poll
  interval (60 s at the default) the chip reads "held · <n>
  free" without a reload, and INFO "memory hold" appears. Lower the floor
  to 0. Pass: within 5 s a cycle starts and the chip returns to its
  polling text, without a reload.
- M6 `peak`: one manual poll with Process Explorer showing the claude
  pid's Peak Private Bytes. Pass: the logged `peak_commit_bytes` is within
  15% of it. A larger gap does not fail the build; it is recorded and
  §4.5's hypothesis is marked refuted, which opens the follow-up in §12.
- M7 `density`: `VITE_MOCK_BACKEND=1 npm run dev`, DPR 1. Pass: at
  innerWidth 756 the layout is narrow and at 757 full; at 572 cards and at
  573 narrow; no horizontal scrollbar at 757 or 573; header labels sit over
  their row columns at 757 and 1600. At 1600x900, open the first row's
  chart: the row top is at the viewport top, the x-axis is visible without
  scrolling, row top to drawer bottom measures 900 ± 1 px in DevTools, and
  the sparkline cell is wider than at 820. At 573x240: the open chart is
  at least 72 px tall and the y labels do not overlap. Then Settings, Text
  size "largest" (CSS zoom 1.22, not browser Ctrl+=, which is page zoom):
  the 1600x900 measure holds, the edges sit at innerWidth 923/924 and
  699/700, and at 700x240 the chart is 72 local px. Text size "small"
  (0.92): the 1600x900 measure holds. At 360 px in the cards layout,
  three rings sit on one line. Repeat the 757/573 scrollbar check at
  DPR 1.25 (browser zoom 125%, the one place page zoom is meant).
- M8 `logoff`: sign out with the window closed to the tray, then check that
  window state survives. UNSURE whether Exit is reached on logoff (#3474).
- M9 `memory-floor-field`: in the mock, the Memory floor field rejects
  65537 with the rollback, and accepts 0.

## 11. Documentation

- README: the memory floor setting and the held chip; close-to-tray frees
  the window's memory; the system font.
- CHANGELOG entry for tauri 2.12 and window-state.

## 12. Out of scope

- Palette changes, new chart types, commit headroom in the UI, automated
  visual regression (§2). The native title bar is kept.
- Measuring the children of `claude.exe` (§4.5): an M6 gap needs a
  follow-up design.

### Accepted residue (Minors not fixed in this revision)

- Sub-pixel rounding at the exact breakpoints on fractional DPR (r1 test
  M4): UNSURE whether Chromium rounds toward a 1 px scrollbar. M7 checks at
  DPR 1.25; a failure is a design finding, and an unmeasured slack
  constant now would be a guess.
- The last second of a poll's growth (F7 null alternative, a post-exit
  read on a handle opened at spawn): unresearched; the 1 s poll with a
  fixed deadline is kept, and M6 decides whether the gap matters.
- `useChartHeight` and the Ring/Sparkline markup have no automated test:
  vitest runs in `node` with no DOM, and adding jsdom for three hooks is
  a toolchain change. The arithmetic is in `chartHeightPx` (tested); the
  wiring and the scroll-on-open are M7.
- `window_open`/kick wiring inside `on_window_event` and
  `show_main_window` needs a Tauri runtime; the helpers they call are
  tested (commands.rs), the two call lines are M1/M4.
- useDashboard/useSystem iterating the events.ts arrays (r2 test M-4b):
  hooks have no DOM test layer (above). events.test pins the names; M5
  (chip changes without reload) and the existing refresh behaviour cover
  the iteration.

## Revision 2 (2026-10-06)

Inputs: review/spec-arch-r1.md (F), review/spec-test-r1.md (C/I/M).

| Finding | Action | Location |
|---|---|---|
| F1 Critical | `memory_hold_changed` event, `memory:hold` refetch, cross-language name test | §4.3, §4.7, §10 |
| F2 = C1 Critical | Chart height measured in JS (`chartHeightPx`), container query dropped | §2 D10, §7, §10 |
| F3 Important | Probe re-read before every spawn; `hold_mid_cycle`; bypass exempt | §4.2, §4.3a, §10 |
| F4 = I2, I1 Important | `.thead` named; `shellVars()` custom properties; css-contract.test | §2 D9, §6, §10 |
| F5 = I3 Important | Spark `minmax(76px,1fr)`; ring sizing; goal 6 range | §1.6, §6, §7 |
| I4 = F6 | Busy branch deleted | §4.3b |
| I5-I8 Important | Shared `MemoryProbe` + `sampler_step` + sampler_loop.rs; peak test in runner_guard.rs and `cycle_finished(peak)`; Startup-then-Presence hold setup; `after_panic(panics, window_open)` | §4.3-§4.5, §5.3, §10 |
| I9 Important | Manual checks pass/fail with numbers; M9 added | §10 Manual |
| F7-F11 Minor | Pinned `sleep_until` loop (F7); `driver_relevant_changed` (F8 = M13); WARN/INFO on transition (F9); ample-memory seed (F10); ring-sm box, `FixedProcess`, 40 sites, `after_panic` (F11) | §4.3-§4.6, §7, §9, §10 |
| M1-M12 | D4 count; anchors and `Core` literals; 10 px arithmetic; TDZ; omitted fields; Refreshed iff changed; skip wrapper; `EXIT_APPROVED`; mockBackend.test; chip figure; `backgroundColor`; `CreateGuard` | §2-§10 |

## Revision 3 (2026-10-06)

Inputs: review/spec-arch-r2.md (F), review/spec-test-r2.md (I/M).

| Finding | Action | Location |
|---|---|---|
| arch F1 Critical | `hold_mid_cycle(.., cycle_closed_gate) -> Option<Gate>` reopens a gate its cut cycle closed (rule 6 at machine.rs:398 is the only gate write; rule 1 blocks it mid-cycle); `CycleInputs.closed_gate`; `gate_changed` emitted; alternative rejected; tests `hold_mid_cycle_reopens_a_gate_its_cycle_closed`, `a_cut_closing_cycle_keeps_the_gate_active` | §2, §4.2, §4.3a, §9, §10 |
| arch F2 Important | `useChartHeight` scrolls the row to the viewport top after its first write; goal 6 and M7 check the x-axis is visible | §1.6, §7, §10 M7 |
| arch F3 = test I-5 Important | M7 uses Settings, Text size "largest" (1.22) and "small" (0.92), edges at 923/924 and 699/700; Ctrl+= only for DPR 1.25 | §7, §10 M7 |
| test I-1 = arch F6 | tests/common `test_core` returns the watch receiver, kept alive by both harnesses | §3, §10 |
| test I-2 Important | Floor tests go through `core_set_settings`, covering the commands.rs:367 send | §10 driver_loop |
| test I-3 Important | `EventSink::settings_applied` (emits `settings:applied`, also fixes the UI not seeing a backoff reset) is the barrier; sampler negatives use a two-sample kick barrier | §4.3, §4.3b, §10 |
| test I-4 Important | Peak tests run `FAKE_CLAUDE_MODE=slow`, 2 s; assert `Some` and `> 0` | §10 |
| arch F4 Minor | Chart minimum is 573x240 (700x240 at 1.22); cards have no drawers | §1.6, §7, §10 M7 |
| arch F5 Minor | `read_memory` + pure `reading_log`; bypass `None` is never a read; unit test | §4.1, §4.3, §10 |
| arch F7/F8 = test M-1 | `../../src/lib/events.ts`; `SYSTEM_EVENTS`, 7 names | §4.3, §10 |
| arch F9 Minor | Event emitted after `publish()` | §4.3 |
| arch F10 Minor | `Win32_Foundation` feature added | §4.1 |
| test M-2 | Ring constants moved to layout.ts; chart.ts imports layout.ts only | §6, §7 |
| test M-3 | `zoom` prop chain; `useLayoutEffect` with a direct style write | §7 |
| test M-4 | `shellStyle(zoom)` pure and tested; `core_get_dashboard` assertion; literal updates noted; hook iteration is residue | §6, §10, §12 |
| test M-5 | M5 needs gate Active, 60 s; M1 DEBUG on, 300 ms, click right after close | §10 Manual |
| test M-6 | tests/common/mod.rs shared by both crates | §10 |

## Revision 4 (2026-10-06)

Inputs: review/spec-arch-r3.md (F), review/spec-test-r3.md (I/M).

- arch F1 Important: scroll target is the outer `.row` (AccountRow.tsx:85) via a `rowRef` prop beside `zoom`; `closest(".row")` rejected; wrapper description corrected (§7).
- arch F2 Minor: `read_memory(probe, lost)` is a free fn in memory.rs; `MemoryGuard { probe, floor_bytes, lost: Arc<AtomicBool> }` carries the Driver's shared flag into `run_cycle`; `read_memory_sets_and_clears_the_shared_flag` added (§4.1, §4.3, §4.3a, §10 memory.rs).
- test I-1 Important: `the_floor_is_read_from_the_settings_watch` sends 4096 MB before `run_sampler` is spawned (§10 sampler_loop).
- test M-1: driver.rs test module defines `AmpleMemory`; all seven `Driver::new` sites pass it (§10 driver_loop).
- test M-2: `FakeMemory::new()` is the one constructor and `script` the only setter; no `dead_code` allow (§10 sampler_loop).
- test M-3: renamed `a_cycle_reports_a_peak`; it proves wiring only, the fold is owned by the `merge_peak_*` unit tests (§10 driver_loop).
- test M-4: `a_cycle_stops_before_the_next_spawn_when_memory_drops` uses the Startup-then-Presence setup (§10 driver_loop).
- test M-5: one `EVT_*` const per event; `FRONTEND_EVENT_NAMES` and every emit site use them; §6 ring-variable pointer fixed (§4.3, §6).
