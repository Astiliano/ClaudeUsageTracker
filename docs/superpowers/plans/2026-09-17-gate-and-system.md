# Gate Reconciliation, Claude Process Usage, Stay-On-Top Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the header chip reflect the process gate on every trigger and within one sample interval of Claude Code starting, show what the Claude processes cost in CPU and memory, and put "on top" one click away in every layout.

**Architecture:** `Machine::decide` gains a `Presence` trigger and reconciles the gate on any `Run` that carries a process answer, so Startup, Manual and AccountChanged stop deciding blind. A new 5-second sampler task owns its own `sysinfo::System`, publishes `SystemStats` into `Core`, and fires a `presence` trigger on the 0-to-non-zero edge; the driver re-probes before deciding, so a racy sample can only cause a skip, never an unwanted poll. The frontend reads the sample through a new `get_system` command and renders a second header line of small meters.

**Tech Stack:** Rust 2021 (rust-version 1.96), Tauri 2.11, tokio 1 (time/process/sync/macros/rt), sysinfo 0.39, rusqlite 0.40, tracing 0.1; React 19 with TypeScript, Vite, vitest.

**Spec:** `docs/superpowers/specs/2026-09-17-gate-and-system-design.md`

## Global Constraints

- Four gates must pass at the end of every task, in this order:
  - `cargo test --manifest-path src-tauri/Cargo.toml`
  - `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
  - `npm test`
  - `npm run build`
- TDD throughout: write the failing test, run it and see it fail, write the minimal implementation, run it and see it pass, commit.
- No `any` types. No `!` non-null assertions. No silent catches. Every async call has error handling. Every timer, listener and subscription is cleaned up.
- `-D warnings` means an unused import is a build failure. Import test-only types inside `mod tests`, never at module scope.
- Spec 5.1: the driver is the only writer of `DriverStatus`. Commands read the published snapshot and never hold a `Machine` handle.
- LOAD-BEARING INVARIANT (`src-tauri/src/scheduler/driver.rs:680-688`): only the driver task calls `Machine::decide` and `begin_cycle`, and never concurrently. Do not call either from a spawned task, and never drop a `CycleToken` while holding the machine guard.
- Rust test conventions: `machine.rs` tests use the `NOW` constant, `ids(&[..])`, `accounts()`, `run_accounts(&d)` and `Machine::new()`; `driver.rs` tests use `test_settings()`, `test_core()`, `test_driver(core, tmp.path())`, `SilentEvents`, `IdleProcess` and `FakeBinary`.
- TypeScript tests are vitest files in `src/lib/*.test.ts` only, node environment, no DOM. Components and hooks are verified by the Playwright pass in Task 14, not by unit tests.
- Every commit message ends with these two lines:

```
Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
```

- Serena is available for Rust navigation only. TypeScript files are read natively.
- Branch: `gate-and-system`.

## File Structure

| File | Responsibility |
|---|---|
| `src-tauri/src/process.rs` (modify) | `matches_claude`, `ProcView`, `Exclusion`, `is_claude_running`. The single definition of "a Claude process that is not ours", shared by the gate and the sampler. |
| `src-tauri/src/scheduler/machine.rs` (modify) | Pure decision rules. Gains `Trigger::Presence`, `SkipReason::AlreadyActive`, and gate reconciliation for every trigger that carries an answer. |
| `src-tauri/src/scheduler/triggers.rs` (modify) | One `Notify` per trigger kind. Gains the `presence` channel. |
| `src-tauri/src/scheduler/driver.rs` (modify) | The only caller of `decide`/`begin_cycle`. Gains `probe_if_free`, the presence arm, `presence_deferred`, the shutdown guard, and `EventSink::system_sampled`. |
| `src-tauri/src/system.rs` (create) | `ClaudeStats`, `SystemStats`, `Sampled`, `Sampler`, `aggregate`, `presence_edge`, `run_sampler`. Owns its own `System`; never touches the gate's. |
| `src-tauri/src/commands.rs` (modify) | `SystemSlot`, `Core.system`, `lock_system`, `core_get_system`, the `get_system` command. |
| `src-tauri/src/lib.rs` (modify) | Wiring only: build `pid_slot`, pass it to both the driver and the sampler, register `get_system`. |
| `src-tauri/src/tray.rs` (modify) | `TauriEvents::system_sampled` emits `system:sampled`. |
| `src/lib/types.ts` (modify) | `ClaudeStats`, `SystemStats`, `SystemReport` mirroring the Rust shapes. |
| `src/lib/system.ts` (create) | Pure presentation: `formatBytes`, `memPct`, `isStale`, `systemLine`, `processCountSuffix`. Every rendering decision lives here so every state is unit-tested. |
| `src/lib/present.ts` (modify) | `chipFor` gains the count suffix; `countPlacement` decides chip versus line. |
| `src/lib/gauge.ts` (modify) | `RING_SIZES` adds the `sm` geometry. |
| `src/components/Ring.tsx` (modify) | Discriminated props so `sm` takes no label. |
| `src/components/SystemLine.tsx` (create) | Dumb renderer for `systemLine`'s output. No state logic. |
| `src/components/Header.tsx` (modify) | Renders `SystemLine`, routes the count to chip or line, hosts the "on top" button. |
| `src/hooks/useSystem.ts` (create) | Loads `get_system`, subscribes to `system:sampled`, sequence-guarded like `useDashboard`. |
| `src/App.tsx` (modify) | Owns `useSystem`, passes report/error/now and the stay-on-top handler to `Header`. |
| `src/components/Settings.tsx` (modify) | Loses the "Keep window on top" toggle. |
| `src/lib/mockBackend.ts` (modify) | `get_system`, the `?mockSystem` flags, the `system:sampled` interval. |
| `src/styles.css` (modify) | `.sysline`, `.sysline-item`, `.sysline-text`, `.sysline-stale`, `.ring-sm`, wrapping `.topbar-actions`. |

---

### Task 1: `process.rs` — `ProcView` and the shared `Exclusion`

Spec §4.1. Today `is_claude_running` excludes only the poll child's pid. The sampler needs the same matcher plus one more rule, so the rule moves into one place both readers use. The start-time clause exists because Windows keeps reporting `th32ParentProcessID` after the parent has died, so a recycled pid must not hide a real session.

**Files:**
- Modify: `src-tauri/src/process.rs`
- Modify: `src-tauri/src/scheduler/driver.rs` (Step 5 only: a temporary compile fix in `SysinfoProbe::claude_running`, replaced by Task 4)
- Test: `src-tauri/src/process.rs` (the existing `mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub struct ProcView { pub pid: u32, pub parent: Option<u32>, pub start_time: u64, pub name: String, pub cmd: Vec<String>, pub rss_bytes: u64, pub cpu: f32 }`
  - `impl From<&sysinfo::Process> for ProcView`
  - `pub struct Exclusion { pub self_pid: u32, pub self_started_at: u64, pub poll_child: Option<u32> }`
  - `pub fn counts(&self, view: &ProcView) -> bool` on `Exclusion`
  - `pub fn is_claude_running(sys: &mut System, exclusion: &Exclusion) -> bool`
  - `pub fn matches_claude(name: &str, cmd: &[String]) -> bool` (unchanged)
  - `pub fn new_system() -> AppResult<System>` (unchanged)

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src-tauri/src/process.rs`:

```rust
    fn view(pid: u32, parent: Option<u32>, start_time: u64, name: &str, cmd: &[&str]) -> ProcView {
        ProcView {
            pid,
            parent,
            start_time,
            name: name.to_string(),
            cmd: cmd.iter().map(|s| s.to_string()).collect(),
            rss_bytes: 0,
            cpu: 0.0,
        }
    }

    fn exclusion(poll_child: Option<u32>) -> Exclusion {
        Exclusion { self_pid: 100, self_started_at: 5_000, poll_child }
    }

    #[test]
    fn a_user_claude_counts() {
        let v = view(200, Some(1), 6_000, "claude.exe", &["claude.exe"]);
        assert!(exclusion(None).counts(&v));
    }

    #[test]
    fn an_npm_form_node_counts() {
        let v = view(
            201,
            Some(1),
            6_000,
            "node.exe",
            &["node.exe", "C:\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js"],
        );
        assert!(exclusion(None).counts(&v));
    }

    #[test]
    fn the_poll_child_is_excluded_by_pid() {
        let v = view(300, Some(100), 6_000, "claude.exe", &["claude.exe"]);
        assert!(!exclusion(Some(300)).counts(&v));
    }

    #[test]
    fn a_child_of_this_app_started_after_it_is_excluded_even_without_a_pid_slot() {
        let v = view(301, Some(100), 6_000, "claude.exe", &["claude.exe"]);
        assert!(
            !exclusion(None).counts(&v),
            "the spawn-to-pid_slot race must not let our own child latch the gate"
        );
    }

    #[test]
    fn a_session_older_than_this_app_counts_even_if_its_parent_pid_was_recycled() {
        let v = view(302, Some(100), 4_999, "claude.exe", &["claude.exe"]);
        assert!(
            exclusion(None).counts(&v),
            "Windows reports th32ParentProcessID after the parent dies; a recycled pid must not hide a real session"
        );
    }

    #[test]
    fn a_zero_self_started_at_falls_back_to_the_plain_parent_check() {
        let e = Exclusion { self_pid: 100, self_started_at: 0, poll_child: None };
        let child = view(303, Some(100), 1, "claude.exe", &["claude.exe"]);
        let other = view(304, Some(7), 1, "claude.exe", &["claude.exe"]);
        assert!(!e.counts(&child));
        assert!(e.counts(&other));
    }

    #[test]
    fn a_non_claude_process_never_counts() {
        let v = view(400, Some(1), 6_000, "code.exe", &["code.exe", "--wait"]);
        assert!(!exclusion(None).counts(&v));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml process::tests`
Expected: FAIL with `cannot find type ProcView in this scope` and `cannot find type Exclusion in this scope`.

- [ ] **Step 3: Write the minimal implementation**

In `src-tauri/src/process.rs`, add after `matches_claude`:

```rust
/// The slice of `sysinfo::Process` both the gate and the sampler read.
/// Built once per process per refresh so the matching rule has a single
/// input shape and cannot drift between the two readers.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcView {
    pub pid: u32,
    pub parent: Option<u32>,
    /// sysinfo's seconds-since-epoch process start time; 0 when the process
    /// could not be opened.
    pub start_time: u64,
    pub name: String,
    pub cmd: Vec<String>,
    pub rss_bytes: u64,
    pub cpu: f32,
}

impl From<&sysinfo::Process> for ProcView {
    fn from(p: &sysinfo::Process) -> ProcView {
        ProcView {
            pid: p.pid().as_u32(),
            parent: p.parent().map(|pp| pp.as_u32()),
            start_time: p.start_time(),
            name: p.name().to_string_lossy().to_string(),
            cmd: p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy().to_string())
                .collect(),
            rss_bytes: p.memory(),
            cpu: p.cpu_usage(),
        }
    }
}

/// Which processes are *ours* and must not be counted (spec §4.1).
///
/// `poll_child` is the live poll child from `pid_slot`. The parent clause
/// covers the window in which the child has been spawned but `pid_slot` has
/// not been written yet. The start-time clause keeps that from misfiring:
/// Windows reports `th32ParentProcessID` even after the parent has exited,
/// so a `claude.exe` older than this app whose original parent's pid was
/// later reused by this app is still a real session and still counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exclusion {
    pub self_pid: u32,
    pub self_started_at: u64,
    pub poll_child: Option<u32>,
}

impl Exclusion {
    pub fn counts(&self, view: &ProcView) -> bool {
        if !matches_claude(&view.name, &view.cmd) {
            return false;
        }
        if Some(view.pid) == self.poll_child {
            return false;
        }
        let ours = view.parent == Some(self.self_pid) && view.start_time >= self.self_started_at;
        !ours
    }
}
```

Replace the body of `is_claude_running` with:

```rust
/// True when any Claude Code process that is not ours is running.
pub fn is_claude_running(sys: &mut System, exclusion: &Exclusion) -> bool {
    let started = Instant::now();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_cmd(UpdateKind::OnlyIfNotSet),
    );

    let mut matched: Vec<u32> = Vec::new();
    for proc_ in sys.processes().values() {
        let view = ProcView::from(proc_);
        if exclusion.counts(&view) {
            matched.push(view.pid);
        }
    }

    debug!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        matched_pids = ?matched,
        self_pid = exclusion.self_pid,
        poll_child = ?exclusion.poll_child,
        "process gate check"
    );
    !matched.is_empty()
}
```

`Pid` is no longer used anywhere in the file, and `-D warnings` rejects an unused import, so the `sysinfo` import line becomes exactly:

```rust
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
```

The `process gate check` line keeps its name, its level and its purpose; only its fields follow the new input, since there is no longer an `exclude_pid` argument to name. It gains no `trigger` field: spec §7 is explicit that which trigger spent the check is on the line above it, in `decision skipped` or `gate changed`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml process::tests`
Expected: PASS, including the nine pre-existing `matches_claude` tests, which are untouched.

- [ ] **Step 5: Run all four gates**

Run each in turn:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Expected: `cargo test` and `cargo clippy` FAIL at this point only if `driver.rs` still calls `is_claude_running` with the old signature. Fix that one call site in `SysinfoProbe::claude_running` now, minimally, so the crate compiles:

```rust
impl ProcessProbe for SysinfoProbe {
    fn claude_running(&self, exclude_pid: Option<u32>) -> bool {
        let mut sys = self.system.lock().unwrap_or_else(PoisonError::into_inner);
        let exclusion = crate::process::Exclusion {
            self_pid: std::process::id(),
            self_started_at: 0,
            poll_child: exclude_pid,
        };
        crate::process::is_claude_running(&mut sys, &exclusion)
    }
}
```

Task 4 replaces `self_started_at: 0` with the cached real value. Re-run all four gates; expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/process.rs src-tauri/src/scheduler/driver.rs
git commit -m "$(cat <<'EOF'
feat(process): share one Claude-process exclusion between gate and sampler

ProcView and Exclusion move the "is this process ours" rule into one place.
The start-time clause keeps a recycled parent pid from hiding a real session.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
EOF
)"
```

---

### Task 2: `machine.rs` — the `Presence` trigger and gate reconciliation

Spec §3.1. Today only a `Timer` moves the gate, so a Startup or Manual poll leaves the chip wrong until the next timer tick. After this task any `Run` that carries a process answer reconciles the gate, with one carve-out: only triggers whose candidate list is the whole enabled set may *close* it, because the run that closes the gate is the final poll.

**Files:**
- Modify: `src-tauri/src/scheduler/machine.rs`
- Test: `src-tauri/src/scheduler/machine.rs` (the existing `mod tests`)

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces:
  - `pub enum Trigger { Timer, Manual, Startup, Presence, AccountChanged(Vec<String>) }` with `Trigger::Presence.as_str() == "presence"`
  - `pub enum SkipReason { Halted, Busy, NoBinary, NoEnabledAccounts, GateIdle, AllBackedOff, AlreadyActive }` with `SkipReason::AlreadyActive.as_str() == "already_active"`
  - `Machine::decide(&mut self, trigger: Trigger, claude_running: Option<bool>, binary_present: bool, halted: bool, enabled: &[String], now: i64) -> Decision` (signature unchanged; rules 4 and 6 change)
  - `pub fn preview_manual(status: &DriverStatus, binary_present: bool, halted: bool, enabled: &[String]) -> Option<SkipReason>` (unchanged)

The decision table this task implements, from spec §3.1 (skips omitted from the candidate column):

| trigger | Idle, Some(true) | Idle, Some(false) | Active, Some(true) | Active, Some(false) | None |
|---|---|---|---|---|---|
| Timer | all, →Active | skip gate_idle | all, — | all, →Idle | skip gate_idle (debug_assert) |
| Presence | all, →Active | skip gate_idle | skip already_active | skip already_active | skip gate_idle (debug_assert) |
| Manual / Startup | all, →Active | all, — | all, — | all, →Idle | all, — |
| AccountChanged | ids∩enabled, →Active | ids∩enabled, — | ids∩enabled, — | ids∩enabled, — | ids∩enabled, — |

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src-tauri/src/scheduler/machine.rs`:

```rust
    #[test]
    fn a_startup_cycle_with_claude_running_lands_on_active() {
        let mut m = Machine::new();
        let d = m.decide(Trigger::Startup, Some(true), true, false, &accounts(), NOW);
        assert_eq!(run_accounts(&d), accounts());
        assert!(matches!(
            d,
            Decision::Run { gate_transition: Some(Gate::Active), .. }
        ));
        assert_eq!(m.gate(), Gate::Active);
    }

    #[test]
    fn a_manual_cycle_with_claude_gone_lands_on_idle() {
        let mut m = Machine::new();
        m.decide(Trigger::Timer, Some(true), true, false, &accounts(), NOW);
        assert_eq!(m.gate(), Gate::Active);

        let d = m.decide(Trigger::Manual, Some(false), true, false, &accounts(), NOW);
        assert_eq!(run_accounts(&d), accounts());
        assert!(matches!(
            d,
            Decision::Run { gate_transition: Some(Gate::Idle), .. }
        ));
        assert_eq!(m.gate(), Gate::Idle);

        assert_eq!(
            m.decide(Trigger::Timer, Some(false), true, false, &accounts(), NOW),
            Decision::Skip(SkipReason::GateIdle),
            "the manual run was the final poll, so the timer must not poll again"
        );
    }

    #[test]
    fn a_manual_cycle_with_claude_gone_while_idle_stays_idle() {
        let mut m = Machine::new();
        let d = m.decide(Trigger::Manual, Some(false), true, false, &accounts(), NOW);
        assert!(matches!(d, Decision::Run { gate_transition: None, .. }));
        assert_eq!(m.gate(), Gate::Idle);
    }

    #[test]
    fn manual_and_startup_without_a_process_answer_keep_the_gate() {
        let mut m = Machine::new();
        let d = m.decide(Trigger::Manual, None, true, false, &accounts(), NOW);
        assert_eq!(run_accounts(&d), accounts());
        assert!(matches!(d, Decision::Run { gate_transition: None, .. }));
        assert_eq!(m.gate(), Gate::Idle, "no answer never moves the gate");

        let d = m.decide(Trigger::Startup, None, true, false, &accounts(), NOW);
        assert_eq!(run_accounts(&d), accounts());
        assert!(matches!(d, Decision::Run { gate_transition: None, .. }));
        assert_eq!(m.gate(), Gate::Idle);
    }

    #[test]
    fn account_changed_opens_the_gate_but_never_closes_it() {
        let mut m = Machine::new();
        let d = m.decide(
            Trigger::AccountChanged(ids(&["a"])),
            Some(true),
            true,
            false,
            &accounts(),
            NOW,
        );
        assert_eq!(run_accounts(&d), ids(&["a"]));
        assert!(matches!(
            d,
            Decision::Run { gate_transition: Some(Gate::Active), .. }
        ));
        assert_eq!(m.gate(), Gate::Active);

        let d = m.decide(
            Trigger::AccountChanged(ids(&["a"])),
            Some(false),
            true,
            false,
            &accounts(),
            NOW,
        );
        assert_eq!(run_accounts(&d), ids(&["a"]));
        assert!(matches!(d, Decision::Run { gate_transition: None, .. }));
        assert_eq!(
            m.gate(),
            Gate::Active,
            "a subset poll must not consume the final poll"
        );
    }

    #[test]
    fn an_account_changed_that_finds_claude_gone_does_not_consume_the_final_poll() {
        let mut m = Machine::new();
        m.decide(Trigger::Timer, Some(true), true, false, &accounts(), NOW);
        m.decide(
            Trigger::AccountChanged(ids(&["a"])),
            Some(false),
            true,
            false,
            &accounts(),
            NOW,
        );

        let d = m.decide(Trigger::Timer, Some(false), true, false, &accounts(), NOW);
        assert_eq!(
            run_accounts(&d),
            accounts(),
            "the final poll still covers every enabled account"
        );
        assert!(matches!(
            d,
            Decision::Run { gate_transition: Some(Gate::Idle), .. }
        ));
    }

    #[test]
    fn presence_runs_only_from_idle_with_claude_running() {
        // Idle + running: run everything and open the gate.
        let mut m = Machine::new();
        let d = m.decide(Trigger::Presence, Some(true), true, false, &accounts(), NOW);
        assert_eq!(run_accounts(&d), accounts());
        assert!(matches!(
            d,
            Decision::Run { gate_transition: Some(Gate::Active), .. }
        ));
        assert_eq!(m.gate(), Gate::Active);

        // Active + running, and Active + not running: nothing to do.
        let d = m.decide(Trigger::Presence, Some(true), true, false, &accounts(), NOW);
        assert_eq!(d, Decision::Skip(SkipReason::AlreadyActive));
        assert_eq!(m.gate(), Gate::Active);
        let d = m.decide(Trigger::Presence, Some(false), true, false, &accounts(), NOW);
        assert_eq!(d, Decision::Skip(SkipReason::AlreadyActive));
        assert_eq!(m.gate(), Gate::Active, "a skip never moves the gate");

        // Idle + not running: the sampler and the fresh probe disagreed.
        let mut m = Machine::new();
        let d = m.decide(Trigger::Presence, Some(false), true, false, &accounts(), NOW);
        assert_eq!(d, Decision::Skip(SkipReason::GateIdle));
        assert_eq!(m.gate(), Gate::Idle);
    }

    #[test]
    fn presence_respects_backoff() {
        let mut m = Machine::new();
        m.record("a", &plain_failure(), NOW);
        m.record("b", &plain_failure(), NOW);
        let d = m.decide(
            Trigger::Presence,
            Some(true),
            true,
            false,
            &accounts(),
            NOW + 1000,
        );
        assert_eq!(d, Decision::Skip(SkipReason::AllBackedOff));
        assert_eq!(m.gate(), Gate::Idle, "a skipped presence never opens the gate");
    }
```

Extend `wire_forms_are_snake_case` with two lines, immediately after the `Trigger::Startup` assertion and after the `SkipReason::AllBackedOff` assertion respectively:

```rust
        assert_eq!(Trigger::Presence.as_str(), "presence");
```

```rust
        assert_eq!(SkipReason::AlreadyActive.as_str(), "already_active");
```

Extend `a_timer_without_a_process_answer_skips_as_gate_idle` with the presence case, inside the existing `cfg!(debug_assertions)` guard, after the timer assertion:

```rust
        assert_eq!(
            m.decide(Trigger::Presence, None, true, false, &accounts(), NOW),
            Decision::Skip(SkipReason::GateIdle)
        );
```

That block only runs under `cargo test --release`: the existing test returns early when `cfg!(debug_assertions)` is true, because rule 4's `debug_assert!` would otherwise panic. In a debug build the assert *is* the guard, and in a release build this assertion pins the fallback. The pattern is pre-existing (`src-tauri/src/scheduler/machine.rs:574-583`) and sanctioned by the spec, so do not "fix" the early return.

Rename the pinned test `manual_and_startup_ignore_the_gate` to `manual_and_startup_without_a_process_answer_keep_the_gate` by deleting the old function outright: the new test of that name above replaces it, and its old body asserts the behaviour this task deliberately changes.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml scheduler::machine`
Expected: FAIL with `no variant or associated item named Presence found for enum Trigger` and `no variant named AlreadyActive found for enum SkipReason`.

- [ ] **Step 3: Write the minimal implementation**

In `src-tauri/src/scheduler/machine.rs`, add the variant to `Trigger` and its wire form:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    Timer,
    Manual,
    Startup,
    /// The sampler saw the Claude process count go from zero to non-zero
    /// (spec §4.3). It does not bypass backoff.
    Presence,
    AccountChanged(Vec<String>),
}

impl Trigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Trigger::Timer => "timer",
            Trigger::Manual => "manual",
            Trigger::Startup => "startup",
            Trigger::Presence => "presence",
            Trigger::AccountChanged(_) => "account_changed",
        }
    }

    /// `Manual` and `AccountChanged` both ignore and reset backoff.
    fn bypasses_backoff(&self) -> bool {
        matches!(self, Trigger::Manual | Trigger::AccountChanged(_))
    }

    /// True when this trigger's candidate list is the whole enabled set, so
    /// a run of it may be the final poll and may close the gate (spec §3.1
    /// rule 6). `AccountChanged` polls a subset and is therefore excluded.
    fn polls_every_enabled_account(&self) -> bool {
        matches!(self, Trigger::Timer | Trigger::Manual | Trigger::Startup)
    }
}
```

Add the skip reason:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    Halted,
    Busy,
    NoBinary,
    NoEnabledAccounts,
    GateIdle,
    AllBackedOff,
    /// A presence wake arrived while the gate was already open.
    AlreadyActive,
}

impl SkipReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkipReason::Halted => "halted",
            SkipReason::Busy => "busy",
            SkipReason::NoBinary => "no_binary",
            SkipReason::NoEnabledAccounts => "no_enabled_accounts",
            SkipReason::GateIdle => "gate_idle",
            SkipReason::AllBackedOff => "all_backed_off",
            SkipReason::AlreadyActive => "already_active",
        }
    }
}
```

Replace rule 4's candidate block in `decide` (the `let candidates: Vec<String> = match &trigger { ... }` expression) with:

```rust
        // 4. Candidate list.
        let mut next_gate = self.gate;
        let candidates: Vec<String> = match &trigger {
            Trigger::Timer | Trigger::Presence => {
                let running = match claude_running {
                    Some(r) => r,
                    None => {
                        debug_assert!(
                            false,
                            "driver bug: a Timer or Presence decision needs a process-check answer"
                        );
                        return Decision::Skip(SkipReason::GateIdle);
                    }
                };
                match (&trigger, self.gate, running) {
                    // A presence wake is only ever about opening the gate.
                    (Trigger::Presence, Gate::Active, _) => {
                        return Decision::Skip(SkipReason::AlreadyActive)
                    }
                    (_, Gate::Idle, false) => return Decision::Skip(SkipReason::GateIdle),
                    (_, Gate::Idle, true) => next_gate = Gate::Active,
                    (_, Gate::Active, false) => next_gate = Gate::Idle,
                    (_, Gate::Active, true) => {}
                }
                enabled.to_vec()
            }
            Trigger::Manual | Trigger::Startup => {
                match (self.gate, claude_running) {
                    (Gate::Idle, Some(true)) => next_gate = Gate::Active,
                    (Gate::Active, Some(false)) => next_gate = Gate::Idle,
                    _ => {}
                }
                enabled.to_vec()
            }
            Trigger::AccountChanged(ids) => {
                // A subset poll may open the gate but never close it: the
                // run that closes it is the final poll, and the final poll
                // covers every enabled account not in cooldown.
                if let (Gate::Idle, Some(true)) = (self.gate, claude_running) {
                    next_gate = Gate::Active;
                }
                enabled
                    .iter()
                    .filter(|e| ids.iter().any(|i| i == *e))
                    .cloned()
                    .collect()
            }
        };
```

Replace rule 6 with:

```rust
        // 6. A Run with a process answer reconciles the gate (spec §3.1).
        // Only a trigger that polls every enabled account may close it, so
        // the final poll is never spent on a subset.
        let closing = next_gate == Gate::Idle && self.gate == Gate::Active;
        let may_move = next_gate != self.gate
            && (!closing || trigger.polls_every_enabled_account());
        let gate_transition = if may_move {
            self.gate = next_gate;
            Some(next_gate)
        } else {
            None
        };
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml scheduler::machine`
Expected: PASS, including the pre-existing `timer_decision_table_for_all_four_gate_and_running_combinations`, `the_final_poll_happens_exactly_once` and `busy_is_checked_before_the_process_state`.

- [ ] **Step 5: Run all four gates**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Expected: PASS. If `clippy` reports a non-exhaustive match anywhere that matches on `Trigger` or `SkipReason`, add the new variant to that match rather than adding a wildcard arm.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/scheduler/machine.rs
git commit -m "$(cat <<'EOF'
feat(machine): reconcile the gate on every trigger that has an answer

Adds Trigger::Presence and SkipReason::AlreadyActive. Any Run carrying a
process answer opens the gate; only Timer, Manual and Startup may close it,
because the closing run is the final poll and must cover every account.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
EOF
)"
```

---

### Task 3: `triggers.rs` — the presence channel

Spec §3.3. One `Notify` per trigger kind, so repeated wakes coalesce into at most one pending trigger instead of queueing.

**Files:**
- Modify: `src-tauri/src/scheduler/triggers.rs`
- Test: `src-tauri/src/scheduler/triggers.rs` (the existing `mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub fn presence(&self)` on `Triggers`
  - `pub async fn notified_presence(&self)` on `Triggers`

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src-tauri/src/scheduler/triggers.rs`:

```rust
    #[tokio::test]
    async fn presence_coalesces_like_manual() {
        let t = Arc::new(Triggers::new());
        t.presence();
        t.presence();

        tokio::time::timeout(Duration::from_millis(200), t.notified_presence())
            .await
            .expect("first notification");

        let second = tokio::time::timeout(Duration::from_millis(200), t.notified_presence()).await;
        assert!(second.is_err(), "presence wakes must coalesce, not queue");
    }

    #[tokio::test]
    async fn presence_is_its_own_channel() {
        let t = Arc::new(Triggers::new());
        t.presence();
        tokio::time::timeout(Duration::from_millis(200), t.notified_presence())
            .await
            .expect("presence notification");
        let manual = tokio::time::timeout(Duration::from_millis(200), t.notified_manual()).await;
        assert!(manual.is_err(), "presence must not fire the manual channel");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml scheduler::triggers`
Expected: FAIL with `no method named presence found for struct Triggers`.

- [ ] **Step 3: Write the minimal implementation**

In `src-tauri/src/scheduler/triggers.rs`, add the field to the struct:

```rust
#[derive(Debug, Default)]
pub struct Triggers {
    manual: Notify,
    startup: Notify,
    presence: Notify,
    changed: Notify,
    changed_ids: Mutex<HashSet<String>>,
}
```

Add the two methods, `presence` next to `startup` and `notified_presence` next to `notified_startup`:

```rust
    /// Fired by the sampler on the zero-to-non-zero Claude process edge
    /// (spec §4.3). The sampler is the only caller.
    pub fn presence(&self) {
        self.presence.notify_one();
    }
```

```rust
    pub async fn notified_presence(&self) {
        self.presence.notified().await;
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml scheduler::triggers`
Expected: PASS.

- [ ] **Step 5: Run all four gates**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/scheduler/triggers.rs
git commit -m "$(cat <<'EOF'
feat(triggers): add the presence channel

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
EOF
)"
```

---

### Task 4: `driver.rs` — probe before every decision, the presence arm, the shutdown guard

Spec §3.2. Every trigger now decides with a real process answer, taken off the driver task through the existing `blocking` hop. A presence wake that lands while a cycle is running is deferred and re-fired when the cycle ends, because the wake is edge-triggered and the count stays non-zero afterwards, so losing it would cost a full interval.

**Files:**
- Modify: `src-tauri/src/scheduler/driver.rs`
- Modify: `src-tauri/src/tray.rs:291-318` (the `EventSink for TauriEvents` impl)
- Modify: `src-tauri/src/lib.rs` (the `Driver::new` call in `setup`, to compile only)
- Test: `src-tauri/src/scheduler/driver.rs` (the existing `mod tests`)

**Interfaces:**
- Consumes: `crate::process::{Exclusion, is_claude_running}` (Task 1); `Trigger::Presence`, `SkipReason::AlreadyActive` (Task 2); `Triggers::presence`, `Triggers::notified_presence` (Task 3).
- Produces:
  - `Driver::new(core: Arc<Core>, events: Arc<dyn EventSink>, process: Arc<dyn ProcessProbe>, binary: Arc<dyn BinaryProbe>, shutdown: CancellationToken, pid_slot: Arc<AtomicU32>) -> Driver` (sixth parameter is new)
  - `async fn probe_if_free(&self) -> Option<bool>` (private; `None` when shutting down, busy, or the hop failed)
  - `async fn handle_presence(&self, settings: &UserSettings, done_tx: &UnboundedSender<u64>, deferred: &mut bool) -> Option<LiveCycle>` (private, mirrors `handle_account_changed`)
  - `fn flush_deferred_presence(core: &Core, deferred: &mut bool)` (private, module level)
  - `fn system_sampled(&self)` on `pub trait EventSink`

`ProcessProbe::claude_running` and `crate::process::is_claude_running` keep the signatures Task 1 left them with. Spec §7 is explicit that the `process gate check` DEBUG line is unchanged: which trigger spent the check is on the line above it, in `decision skipped` or `gate changed`.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src-tauri/src/scheduler/driver.rs`. First a counting probe, next to `IdleProcess`:

```rust
    /// Counts how many process checks were actually spent.
    struct CountingProcess {
        running: bool,
        calls: Arc<AtomicU64>,
    }
    impl ProcessProbe for CountingProcess {
        fn claude_running(&self, _exclude_pid: Option<u32>) -> bool {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.running
        }
    }
```

Then the tests:

```rust
    #[tokio::test]
    async fn probe_if_free_spends_no_process_check_while_busy() {
        let (tmp, core, _id) = test_core();
        let calls = Arc::new(AtomicU64::new(0));
        let driver = Driver::new(
            Arc::clone(&core),
            Arc::new(SilentEvents),
            Arc::new(CountingProcess { running: true, calls: Arc::clone(&calls) }),
            Arc::new(FakeBinary(tmp.path().join("claude.exe"))),
            CancellationToken::new(),
            Arc::new(AtomicU32::new(0)),
        );

        {
            let _token = begin_cycle(&driver.machine, 1);
            assert_eq!(driver.probe_if_free().await, None, "busy must short-circuit");
            assert_eq!(calls.load(Ordering::SeqCst), 0, "no check may be spent while busy");
        }

        assert_eq!(driver.probe_if_free().await, Some(true));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn probe_if_free_spends_no_process_check_once_shutting_down() {
        let (tmp, core, _id) = test_core();
        let calls = Arc::new(AtomicU64::new(0));
        let shutdown = CancellationToken::new();
        let driver = Driver::new(
            Arc::clone(&core),
            Arc::new(SilentEvents),
            Arc::new(CountingProcess { running: true, calls: Arc::clone(&calls) }),
            Arc::new(FakeBinary(tmp.path().join("claude.exe"))),
            shutdown.clone(),
            Arc::new(AtomicU32::new(0)),
        );

        shutdown.cancel();
        assert_eq!(driver.probe_if_free().await, None);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "no walk may be spent in the exit window"
        );
    }

    #[tokio::test]
    async fn startup_with_claude_running_publishes_active() {
        let (tmp, core, _id) = test_core();
        let driver = Driver::new(
            Arc::clone(&core),
            Arc::new(SilentEvents),
            Arc::new(IdleProcess),
            Arc::new(FakeBinary(tmp.path().join("claude.exe"))),
            CancellationToken::new(),
            Arc::new(AtomicU32::new(0)),
        );
        let (done_tx, _done_rx) = tokio::sync::mpsc::unbounded_channel::<u64>();

        let cycle = driver
            .decide_and_maybe_run(Trigger::Startup, Some(true), &test_settings(), &done_tx)
            .await
            .expect("startup runs");
        assert_eq!(
            lock_status(&core.status).gate,
            Gate::Active,
            "the chip must be right before the cycle even finishes"
        );
        cycle.cancel.cancel();
        cycle.handle.abort();
        let _ = cycle.handle.await;
    }

    #[tokio::test]
    async fn a_presence_decision_is_skipped_at_debug_when_already_active() {
        let (tmp, core, _id) = test_core();
        let driver = test_driver(Arc::clone(&core), tmp.path());
        let (done_tx, _done_rx) = tokio::sync::mpsc::unbounded_channel::<u64>();

        let cycle = driver
            .decide_and_maybe_run(Trigger::Timer, Some(true), &test_settings(), &done_tx)
            .await
            .expect("timer opens the gate");
        cycle.cancel.cancel();
        cycle.handle.abort();
        let _ = cycle.handle.await;

        assert!(
            driver
                .decide_and_maybe_run(Trigger::Presence, Some(true), &test_settings(), &done_tx)
                .await
                .is_none(),
            "the gate is already open, so a presence wake polls nothing"
        );
    }

    #[tokio::test]
    async fn a_trigger_after_shutdown_starts_no_cycle() {
        let (tmp, core, _id) = test_core();
        let shutdown = CancellationToken::new();
        let driver = Driver::new(
            Arc::clone(&core),
            Arc::new(SilentEvents),
            Arc::new(IdleProcess),
            Arc::new(FakeBinary(tmp.path().join("claude.exe"))),
            shutdown.clone(),
            Arc::new(AtomicU32::new(0)),
        );
        let (done_tx, _done_rx) = tokio::sync::mpsc::unbounded_channel::<u64>();

        // A value only a publish would destroy. `publish_status` recomputes
        // `busy` from the machine, which has no cycle, so a `busy: true`
        // planted here survives exactly as long as nothing publishes.
        // `stalled_at` would NOT work: `publish_status` deliberately carries
        // it across from the slot, so it survives a publish too.
        {
            let mut status = lock_status(&core.status);
            status.busy = true;
            status.stalled_at = Some(1);
        }
        let before = lock_status(&core.status).clone();

        shutdown.cancel();
        assert!(
            driver
                .decide_and_maybe_run(Trigger::Manual, Some(true), &test_settings(), &done_tx)
                .await
                .is_none(),
            "a wake landing in the exit window must not start a cycle"
        );
        assert_eq!(
            *lock_status(&core.status),
            before,
            "the guard returns before `decide`, so no snapshot is published"
        );
        assert!(
            lock_status(&core.status).busy,
            "the planted value is what proves it: a publish would have cleared it"
        );
    }

    #[tokio::test]
    async fn a_presence_wake_skipped_while_busy_is_refired_when_the_cycle_ends() {
        let (tmp, core, _id) = test_core();
        let calls = Arc::new(AtomicU64::new(0));
        let driver = Driver::new(
            Arc::clone(&core),
            Arc::new(SilentEvents),
            Arc::new(CountingProcess { running: true, calls: Arc::clone(&calls) }),
            Arc::new(FakeBinary(tmp.path().join("claude.exe"))),
            CancellationToken::new(),
            Arc::new(AtomicU32::new(0)),
        );
        let (done_tx, _done_rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
        let mut presence_deferred = false;

        // A cycle is in flight, so the wake cannot be decided now. The arm
        // itself must set the flag: nothing here sets it by hand.
        {
            let _token = begin_cycle(&driver.machine, 1);
            assert!(
                driver
                    .handle_presence(&test_settings(), &done_tx, &mut presence_deferred)
                    .await
                    .is_none(),
                "busy, so nothing runs"
            );
            assert!(presence_deferred, "a wake consumed while busy must be deferred");
            assert_eq!(
                calls.load(Ordering::SeqCst),
                0,
                "no process check may be spent on a wake that cannot be decided"
            );
        }

        // The cycle has ended; the flush point must re-arm the wake.
        flush_deferred_presence(&core, &mut presence_deferred);
        assert!(!presence_deferred, "the flag is cleared once the wake is re-armed");
        assert!(
            tokio::time::timeout(
                Duration::from_millis(200),
                core.triggers.notified_presence()
            )
            .await
            .is_ok(),
            "the deferred wake must be pending again"
        );
    }

    #[tokio::test]
    async fn a_presence_wake_spends_no_probe_when_the_gate_is_already_open() {
        let (tmp, core, _id) = test_core();
        let calls = Arc::new(AtomicU64::new(0));
        let driver = Driver::new(
            Arc::clone(&core),
            Arc::new(SilentEvents),
            Arc::new(CountingProcess { running: true, calls: Arc::clone(&calls) }),
            Arc::new(FakeBinary(tmp.path().join("claude.exe"))),
            CancellationToken::new(),
            Arc::new(AtomicU32::new(0)),
        );
        let (done_tx, _done_rx) = tokio::sync::mpsc::unbounded_channel::<u64>();

        let cycle = driver
            .decide_and_maybe_run(Trigger::Timer, Some(true), &test_settings(), &done_tx)
            .await
            .expect("timer opens the gate");
        cycle.cancel.cancel();
        cycle.handle.abort();
        let _ = cycle.handle.await;
        let spent = calls.load(Ordering::SeqCst);

        let mut presence_deferred = false;
        assert!(driver
            .handle_presence(&test_settings(), &done_tx, &mut presence_deferred)
            .await
            .is_none());
        assert!(!presence_deferred, "an already-open gate is not a deferral");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            spent,
            "the gate is already open, so no check is spent"
        );
    }
```

No new imports are needed in `mod tests`: `AtomicU32`, `AtomicU64`, `Ordering`, `Arc` and `Duration` are all imported at module scope (`src-tauri/src/scheduler/driver.rs:1-7`) and reach the test module through its `use super::*`. Adding them again would be an unused import and `-D warnings` would reject it. `Gate` is the one name that is *not* imported there; Step 3 adds it at module scope, where the presence arm needs it.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml scheduler::driver`
Expected: FAIL with `this function takes 5 arguments but 6 arguments were supplied`, `no method named handle_presence found for struct Driver`, and `cannot find function flush_deferred_presence in this scope`.

- [ ] **Step 3: Write the minimal implementation**

In `src-tauri/src/scheduler/driver.rs`, add the trait method:

```rust
pub trait EventSink: Send + Sync {
    fn usage_updated(&self, account_id: &str);
    fn cycle_finished(&self);
    fn gate_changed(&self, gate: &str);
    fn poller_stalled(&self, at: i64, cycle_age_ms: u64);
    fn refresh_tray(&self);
    /// The sampler published a fresh `SystemStats` (spec §4.2).
    fn system_sampled(&self);
}
```

Add the empty impl to `SilentEvents` in `mod tests`:

```rust
        fn system_sampled(&self) {}
```

Give `SysinfoProbe` the cached exclusion, replacing the struct and its impls:

```rust
pub struct SysinfoProbe {
    system: Mutex<sysinfo::System>,
    self_pid: u32,
    /// Cached on first use: the app's own start time, so the exclusion can
    /// tell our poll child from a real session whose dead parent's pid the
    /// OS later handed to us.
    self_started_at: OnceLock<u64>,
}

impl SysinfoProbe {
    pub fn new() -> AppResult<SysinfoProbe> {
        Ok(SysinfoProbe {
            system: Mutex::new(crate::process::new_system()?),
            self_pid: std::process::id(),
            self_started_at: OnceLock::new(),
        })
    }
}

impl ProcessProbe for SysinfoProbe {
    fn claude_running(&self, exclude_pid: Option<u32>) -> bool {
        let mut sys = self.system.lock().unwrap_or_else(PoisonError::into_inner);
        let started_at = *self.self_started_at.get_or_init(|| {
            sys.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::All,
                true,
                sysinfo::ProcessRefreshKind::nothing(),
            );
            sys.process(sysinfo::Pid::from_u32(self.self_pid))
                .map(|p| p.start_time())
                .unwrap_or(0)
        });
        let exclusion = crate::process::Exclusion {
            self_pid: self.self_pid,
            self_started_at: started_at,
            poll_child: exclude_pid,
        };
        crate::process::is_claude_running(&mut sys, &exclusion)
    }
}
```

Add `use std::sync::OnceLock;` to the module's imports.

Add the presence flush helper next to `flush_deferred_changes`:

```rust
/// Re-arms a presence wake that was consumed while a cycle was running.
/// The wake is edge-triggered and the process count stays non-zero
/// afterwards, so without this the gate would wait for the next Timer.
fn flush_deferred_presence(core: &Core, deferred: &mut bool) {
    if *deferred {
        *deferred = false;
        core.triggers.presence();
    }
}
```

Change `Driver::new` to take the shared slot:

```rust
    pub fn new(
        core: Arc<Core>,
        events: Arc<dyn EventSink>,
        process: Arc<dyn ProcessProbe>,
        binary: Arc<dyn BinaryProbe>,
        shutdown: CancellationToken,
        pid_slot: Arc<AtomicU32>,
    ) -> Driver {
        Driver {
            core,
            events,
            process,
            binary,
            shutdown,
            pid_slot,
            cycle_generation: AtomicU64::new(0),
            machine: Arc::new(Mutex::new(Machine::new())),
        }
    }
```

Add the probe helper next to `current_pid`:

```rust
    /// The process answer for a decision, or `None` when there is none to
    /// be had: the app is shutting down, a cycle is running, or the hop
    /// failed. Busy is checked before the walk so the app's own poll child
    /// can never latch the gate, and the walk itself goes through the
    /// `blocking` hop
    /// because `AccountChanged` fires on every add, enable, disable and
    /// rescan.
    ///
    /// The extra await is safe: this task is the only caller of `decide`
    /// and `begin_cycle`, a chosen `select!` arm runs to completion before
    /// another is polled, and the only thing that can change busy during
    /// the await is a cycle *ending*, which only makes the answer more
    /// current. A failed hop yields `None`, never a guessed `false`: no
    /// answer leaves the gate alone, whereas a wrong `false` would close it.
    async fn probe_if_free(&self) -> Option<bool> {
        // Every reason for `None` is named here, once, so a caller never has
        // to guess which one it got. Nothing may spend a process walk once
        // the app is closing: the answer could only feed a decision
        // `decide_and_maybe_run` is about to refuse anyway, and a hop the
        // runtime tears down mid-flight would log the WARN below for a
        // failure that is not one.
        if self.shutdown.is_cancelled() {
            debug!(reason = "shutting down", "process check skipped");
            return None;
        }
        if lock_machine(&self.machine).is_busy() {
            debug!(reason = "busy", "process check skipped");
            return None;
        }
        let process = Arc::clone(&self.process);
        let pid = self.current_pid();
        match blocking(move || Ok(process.claude_running(pid))).await {
            Ok(running) => Some(running),
            Err(e) => {
                warn!(error = %e, "process check failed; gate left unchanged");
                None
            }
        }
    }
```

Add the shutdown guard at the top of `decide_and_maybe_run`, before `refresh_binary`:

```rust
        if self.shutdown.is_cancelled() {
            debug!(trigger = trigger.as_str(), "trigger ignored: shutting down");
            return None;
        }
```

Extend the skip logging in `decide_and_maybe_run` so a presence skip is DEBUG, not WARN:

```rust
            Decision::Skip(reason) => {
                match reason {
                    crate::scheduler::machine::SkipReason::GateIdle
                    | crate::scheduler::machine::SkipReason::Busy
                    | crate::scheduler::machine::SkipReason::AlreadyActive => {
                        debug!(reason = reason.as_str(), "decision skipped")
                    }
                    _ => warn!(reason = reason.as_str(), "decision skipped"),
                }
                None
            }
```

Add the trigger field to the gate-changed log line in the same function:

```rust
                if let Some(gate) = gate_transition {
                    info!(gate = gate.as_str(), trigger = reason.as_str(), "gate changed");
                    self.events.gate_changed(gate.as_str());
                }
```

In `run()`, declare the new flag next to `changed_deferred`:

```rust
        let mut presence_deferred = false;
```

Change the pre-loop Startup decision to probe:

```rust
        let startup_answer = self.probe_if_free().await;
        if let Some(cycle) = self
            .decide_and_maybe_run(Trigger::Startup, startup_answer, &settings, &done_tx)
            .await
        {
            live = Some(cycle);
        }
```

Change the Timer arm's body to use the helper:

```rust
                _ = tokio::time::sleep(wait) => {
                    // Busy is checked inside the helper before a process
                    // check is spent, so the app's own child can never
                    // latch the gate. A failed check lands here too, and
                    // the Timer never hands `None` to `decide`.
                    let Some(running) = self.probe_if_free().await else {
                        // probe_if_free has already logged which reason.
                        debug!("timer skipped: no process answer");
                        last_cycle_end = chrono::Utc::now().timestamp_millis();
                        continue;
                    };
                    if let Some(cycle) = self
                        .decide_and_maybe_run(Trigger::Timer, Some(running), &settings, &done_tx)
                        .await
                    {
                        live = Some(cycle);
                    } else {
                        last_cycle_end = chrono::Utc::now().timestamp_millis();
                    }
                }
```

Change the Manual and Startup arms to probe:

```rust
                _ = self.core.triggers.notified_manual() => {
                    let answer = self.probe_if_free().await;
                    if let Some(cycle) = self
                        .decide_and_maybe_run(Trigger::Manual, answer, &settings, &done_tx)
                        .await
                    {
                        live = Some(cycle);
                    }
                }
                _ = self.core.triggers.notified_startup() => {
                    let answer = self.probe_if_free().await;
                    if let Some(cycle) = self
                        .decide_and_maybe_run(Trigger::Startup, answer, &settings, &done_tx)
                        .await
                    {
                        live = Some(cycle);
                    }
                }
```

Add `handle_presence` next to `handle_account_changed`, so the arm's behaviour is a function a test can call rather than a block only the loop can reach:

```rust
    /// One presence wake.
    ///
    /// Three outcomes, in this order. The gate is already open: nothing to
    /// do, and no process check is spent. A cycle is running: the wake is
    /// deferred, because it is edge-triggered and the process count stays
    /// non-zero afterwards, so dropping it would leave the gate shut until
    /// the next Timer. Otherwise probe and decide; a failed probe is a skip,
    /// not a deferral, since the next Timer reconciles the gate anyway.
    async fn handle_presence(
        &self,
        settings: &UserSettings,
        done_tx: &UnboundedSender<u64>,
        deferred: &mut bool,
    ) -> Option<LiveCycle> {
        if lock_machine(&self.machine).gate() == Gate::Active {
            debug!(reason = "already_active", "presence skipped");
            return None;
        }
        if lock_machine(&self.machine).is_busy() {
            debug!(reason = "busy", "presence skipped");
            *deferred = true;
            return None;
        }
        let running = match self.probe_if_free().await {
            Some(r) => r,
            None => {
                // probe_if_free has already logged which reason.
                debug!(reason = "no process answer", "presence skipped");
                return None;
            }
        };
        self.decide_and_maybe_run(Trigger::Presence, Some(running), settings, done_tx)
            .await
    }
```

Add the presence arm immediately after the startup arm:

```rust
                _ = self.core.triggers.notified_presence() => {
                    if let Some(cycle) = self
                        .handle_presence(&settings, &done_tx, &mut presence_deferred)
                        .await
                    {
                        live = Some(cycle);
                    }
                }
```

`Gate` is not currently imported in `driver.rs`, so add it to the existing machine import list at `src-tauri/src/scheduler/driver.rs:11-14`:

```rust
use crate::scheduler::machine::{
    begin_cycle, lock_machine, CycleToken, Decision, DriverStatus, Gate, Machine, Recorded,
    SharedMachine, Trigger,
};
```

Change `handle_account_changed` to probe. Replace its `decide_and_maybe_run` call:

```rust
        let restore = ids.clone();
        let answer = self.probe_if_free().await;
        match self
            .decide_and_maybe_run(Trigger::AccountChanged(ids), answer, settings, done_tx)
            .await
```

Add the presence flush at all three points where `flush_deferred_changes` is already called. After each existing call, add:

```rust
                flush_deferred_presence(&self.core, &mut presence_deferred);
```

The three sites are the backstop reap, the `done_rx` reap, and the watchdog abort. In the watchdog arm, match the existing multi-line call style:

```rust
                                flush_deferred_changes(
                                    &self.core,
                                    &mut changed_deferred,
                                );
                                flush_deferred_presence(
                                    &self.core,
                                    &mut presence_deferred,
                                );
```

In `src-tauri/src/tray.rs`, add to `impl EventSink for TauriEvents`, after `poller_stalled`:

```rust
    fn system_sampled(&self) {
        let _ = self.app.emit("system:sampled", ());
    }
```

Update the `test_driver` fixture in `mod tests` (`src-tauri/src/scheduler/driver.rs:958-966`), which still passes five arguments. Replace its `CancellationToken::new(),` line so the call reads:

```rust
    fn test_driver(core: Arc<Core>, tmp: &std::path::Path) -> Driver {
        Driver::new(
            core,
            Arc::new(SilentEvents),
            Arc::new(IdleProcess),
            Arc::new(FakeBinary(tmp.join("claude.exe"))),
            CancellationToken::new(),
            Arc::new(AtomicU32::new(0)),
        )
    }
```

In `src-tauri/src/lib.rs`, replace the whole `// Scheduler.` block in `setup`. Two things change from what is there now: `pid_slot` is created here rather than inside `Driver::new`, and `events` is cloned into the driver rather than moved, because Task 6 hands the other clone to the sampler. Cloning the `Arc<dyn EventSink>` shares one `TauriEvents` instead of duplicating its tray state:

```rust
            let events: Arc<dyn EventSink> =
                Arc::new(TauriEvents::new(handle.clone(), Arc::clone(&core)));
            let process: Arc<dyn ProcessProbe> = Arc::new(SysinfoProbe::new()?);
            let binary: Arc<dyn BinaryProbe> = Arc::new(RealBinaryProbe);
            let pid_slot = Arc::new(std::sync::atomic::AtomicU32::new(0));
            let driver = Driver::new(
                Arc::clone(&core),
                Arc::clone(&events),
                process,
                binary,
                shutdown.clone(),
                Arc::clone(&pid_slot),
            );
            tauri::async_runtime::spawn(driver.run());
```

Both `events` and `pid_slot` are used here by their `Arc::clone` calls, so neither draws an unused-variable warning while Task 6 is still outstanding.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml scheduler::driver`
Expected: PASS, including the pre-existing `an_account_changed_skipped_while_halted_keeps_its_ids` and `a_busy_account_changed_leaves_the_ids_undrained`.

- [ ] **Step 5: Run all four gates**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/scheduler/driver.rs src-tauri/src/tray.rs src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(driver): probe before every decision, add the presence arm

probe_if_free moves the process walk onto the blocking pool and yields None
rather than a guessed answer. A presence wake consumed while a cycle runs is
deferred and re-fired at the same three flush points account changes use. A
trigger landing in the exit window no longer starts a cycle.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
EOF
)"
```

---

### Task 5: `system.rs` — the sampler

Spec §4.1 and §4.3. A 5-second walk on the blocking pool, with its own `System` so a gate refresh cannot disturb the CPU diff baseline. Priming is three refreshes with two 200 ms gaps because sysinfo's Windows `compute_cpu_usage` stamps `last_update` on every call but seeds the `old_*` baseline only once the interval has elapsed: the first refresh seeds nothing, the second diffs against zero and produces a since-boot average, and only the third is a true diff.

**Files:**
- Create: `src-tauri/src/system.rs`
- Modify: `src-tauri/src/commands.rs` (`SystemSlot`, `lock_system`, the `Core.system` field)
- Modify: `src-tauri/src/lib.rs` (add `pub mod system;`, the `Core` literal)
- Modify: `src-tauri/src/scheduler/driver.rs` (the `test_core()` literal)
- Test: `src-tauri/src/system.rs` (new `mod tests`)

The sampler is the only writer of the slot, so the slot lands with the sampler rather than with the command that reads it. That keeps this task green on all four gates on its own.

**Interfaces:**
- Consumes: `crate::process::{Exclusion, ProcView}` (Task 1); `Triggers::presence` (Task 3); `EventSink::system_sampled` (Task 4).
- Produces:
  - `pub struct SystemSlot { pub stats: Option<SystemStats>, pub stopped: bool }` with `Default`, in `commands.rs`
  - `pub fn lock_system(slot: &Mutex<SystemSlot>) -> MutexGuard<'_, SystemSlot>`, in `commands.rs`
  - `pub system: Arc<Mutex<SystemSlot>>` on `Core`
  - `pub struct ClaudeStats { pub count: u32, pub rss_bytes: u64, pub cpu_pct: Option<f32> }`
  - `pub struct SystemStats { pub sampled_at: i64, pub mem_total_bytes: u64, pub claude: ClaudeStats }`
  - `pub struct Sampled { pub stats: SystemStats, pub elapsed_ms: u64, pub did_prime: bool }`
  - `pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(5)`
  - `pub fn aggregate(procs: &[ProcView], exclusion: &Exclusion, cpus: usize) -> ClaudeStats`
  - `pub fn presence_edge(prev_count: u32, next_count: u32) -> bool`
  - `pub fn after_panic(panics: u32) -> Option<Duration>`
  - `pub struct Sampler` with `pub fn new(pid_slot: Arc<AtomicU32>) -> Sampler` and `pub fn sample(&mut self, now_ms: i64) -> Sampled`
  - `pub async fn run_sampler(core: Arc<Core>, events: Arc<dyn EventSink>, pid_slot: Arc<AtomicU32>, shutdown: CancellationToken)`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/system.rs` containing only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{Exclusion, ProcView};

    fn view(pid: u32, name: &str, rss_bytes: u64, cpu: f32) -> ProcView {
        ProcView {
            pid,
            parent: Some(1),
            start_time: 9_000,
            name: name.to_string(),
            cmd: vec![name.to_string()],
            rss_bytes,
            cpu,
        }
    }

    fn exclusion() -> Exclusion {
        Exclusion { self_pid: 100, self_started_at: 5_000, poll_child: None }
    }

    #[test]
    fn aggregate_sums_only_the_counted_processes() {
        let procs = vec![
            view(200, "claude.exe", 1_000, 10.0),
            view(201, "claude.exe", 2_000, 30.0),
            view(202, "code.exe", 9_999, 90.0),
        ];
        let stats = aggregate(&procs, &exclusion(), 4);
        assert_eq!(stats.count, 2);
        assert_eq!(stats.rss_bytes, 3_000);
        assert_eq!(stats.cpu_pct, Some(10.0), "40 per-core percent over 4 cpus");
    }

    #[test]
    fn aggregate_clamps_at_a_hundred() {
        let procs = vec![view(200, "claude.exe", 1, 800.0)];
        let stats = aggregate(&procs, &exclusion(), 4);
        assert_eq!(stats.cpu_pct, Some(100.0));
    }

    #[test]
    fn aggregate_without_a_cpu_count_reports_no_share() {
        let procs = vec![view(200, "claude.exe", 1, 50.0)];
        assert_eq!(aggregate(&procs, &exclusion(), 0).cpu_pct, None);
    }

    #[test]
    fn aggregate_of_nothing_is_zeros() {
        let stats = aggregate(&[], &exclusion(), 4);
        assert_eq!(stats.count, 0);
        assert_eq!(stats.rss_bytes, 0);
        assert_eq!(stats.cpu_pct, Some(0.0));
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
    /// first call only, and produce a usable memory denominator. Not a
    /// value assertion; the numbers depend on the host.
    #[test]
    fn sample_primes_once_and_returns_usable_figures() {
        let mut sampler = Sampler::new(Arc::new(AtomicU32::new(0)));
        let first = sampler.sample(1_700_000_000_000);
        assert!(first.did_prime, "the first call runs the priming refreshes");
        assert!(first.stats.mem_total_bytes > 0);
        assert!(
            first.stats.claude.cpu_pct.is_some(),
            "priming means the first published share is a real diff"
        );

        let second = sampler.sample(1_700_000_005_000);
        assert!(!second.did_prime, "priming happens once per Sampler");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `pub mod system;` to `src-tauri/src/lib.rs` beside `pub mod store;`, then run:

Run: `cargo test --manifest-path src-tauri/Cargo.toml system::tests`
Expected: FAIL with `cannot find function aggregate in this scope` and `cannot find struct Sampler in this scope`.

- [ ] **Step 3: Write the minimal implementation**

First the slot the sampler writes into. In `src-tauri/src/commands.rs`, add next to `lock_status`:

```rust
/// The sampler's published state. `stopped` is set once, by the sampler's
/// terminal break, so a slot that never received a sample is
/// distinguishable from one that is still warming up.
#[derive(Debug, Clone, Default)]
pub struct SystemSlot {
    pub stats: Option<crate::system::SystemStats>,
    pub stopped: bool,
}

/// A poisoned system mutex means a sampler task panicked; the state itself
/// is still coherent, so recover rather than propagate the panic.
pub fn lock_system(slot: &Mutex<SystemSlot>) -> MutexGuard<'_, SystemSlot> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}
```

`Mutex`, `MutexGuard` and `PoisonError` are already imported there (`src-tauri/src/commands.rs:4`).

Add the field to `Core`, after `status`:

```rust
    /// The sampler's published figures (spec §4.2). Written only by
    /// `run_sampler`; every reader clones it.
    pub system: Arc<Mutex<SystemSlot>>,
```

`Core` is built as an exhaustive literal in three places, which all gain a `system:` line after their `status:` line. How the name is spelled differs per site, because `-D warnings` rejects an import a non-test build does not use:

- `src-tauri/src/lib.rs`, the `setup` closure. This is a non-test literal, so extend the existing import to `use crate::commands::{lock_binary, Core, SharedCore, SystemSlot};` and write:

```rust
                system: Arc::new(Mutex::new(SystemSlot::default())),
```

- `src-tauri/src/scheduler/driver.rs`, `fn test_core()` in `mod tests`. Do **not** touch the module-scope import at `src-tauri/src/scheduler/driver.rs:9`: `SystemSlot` would be used only under `cfg(test)`, so a non-test build would reject it as an unused import. Name it inline instead:

```rust
            system: Arc::new(Mutex::new(crate::commands::SystemSlot::default())),
```

- `src-tauri/src/commands.rs`, `fn core()` in `mod tests`. `SystemSlot` is defined in that file, so `use super::*` already covers it:

```rust
            system: Arc::new(Mutex::new(SystemSlot::default())),
```

Now the module body, above the test module in `src-tauri/src/system.rs`:

```rust
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::{CpuRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::commands::{lock_system, Core};
use crate::process::{Exclusion, ProcView};
use crate::scheduler::driver::EventSink;

/// How often the sampler walks the process table.
pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);

/// Three consecutive panicking samples stop the task for the rest of the run.
const MAX_CONSECUTIVE_PANICS: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClaudeStats {
    /// Claude Code processes other than the poll child and any child of this app.
    pub count: u32,
    /// Sum of their resident memory, bytes.
    pub rss_bytes: u64,
    /// Sum of their CPU usage as a share of the whole machine, 0..=100.
    /// `None` only when the CPU count is unknown (0).
    pub cpu_pct: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemStats {
    /// Epoch milliseconds, so the UI can tell a live figure from a frozen one.
    pub sampled_at: i64,
    /// Denominator for the memory ring only.
    pub mem_total_bytes: u64,
    pub claude: ClaudeStats,
}

/// One call's result. `did_prime` is true only on the call that ran the
/// priming refreshes, unlike the sticky `Sampler::primed` field, so the log
/// shows the priming cost exactly once.
#[derive(Debug, Clone, PartialEq)]
pub struct Sampled {
    pub stats: SystemStats,
    pub elapsed_ms: u64,
    pub did_prime: bool,
}

/// Sums the Claude processes' memory and CPU. `cpu` is sysinfo's per-core
/// percentage, so the machine share is the sum divided by the CPU count.
pub fn aggregate(procs: &[ProcView], exclusion: &Exclusion, cpus: usize) -> ClaudeStats {
    let mut count: u32 = 0;
    let mut rss_bytes: u64 = 0;
    let mut cpu: f32 = 0.0;
    for view in procs {
        if exclusion.counts(view) {
            count = count.saturating_add(1);
            rss_bytes = rss_bytes.saturating_add(view.rss_bytes);
            cpu += view.cpu;
        }
    }
    let cpu_pct = if cpus == 0 {
        None
    } else {
        Some((cpu / cpus as f32).clamp(0.0, 100.0))
    };
    ClaudeStats { count, rss_bytes, cpu_pct }
}

/// The wake condition: Claude Code has just appeared.
pub fn presence_edge(prev_count: u32, next_count: u32) -> bool {
    prev_count == 0 && next_count > 0
}

/// How long to wait after a panicking sample, or `None` to stop for good.
pub fn after_panic(panics: u32) -> Option<Duration> {
    if panics >= MAX_CONSECUTIVE_PANICS {
        None
    } else {
        Some(SAMPLE_INTERVAL)
    }
}

pub struct Sampler {
    system: System,
    self_pid: u32,
    self_started_at: u64,
    pid_slot: Arc<AtomicU32>,
    cpus: usize,
    mem_total_bytes: u64,
    primed: bool,
}

impl Sampler {
    pub fn new(pid_slot: Arc<AtomicU32>) -> Sampler {
        Sampler {
            system: System::new(),
            self_pid: std::process::id(),
            self_started_at: 0,
            pid_slot,
            cpus: 0,
            mem_total_bytes: 0,
            primed: false,
        }
    }

    fn refresh_processes(&mut self) {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_cmd(UpdateKind::OnlyIfNotSet)
                .with_cpu()
                .with_memory(),
        );
    }

    pub fn sample(&mut self, now_ms: i64) -> Sampled {
        let started = Instant::now();
        let did_prime = !self.primed;

        if did_prime {
            // 1. Initialise the CPU list without opening the PDH usage
            //    query. sysinfo divides each process's share by
            //    `cpus().len()`, so this MUST precede the first process
            //    refresh or every `cpu_usage()` reads 0.
            self.system
                .refresh_cpu_specifics(CpuRefreshKind::nothing());
            self.cpus = self.system.cpus().len();
            // 2. Total memory is a constant; read it once.
            self.system.refresh_memory();
            self.mem_total_bytes = self.system.total_memory();
            // 3. First priming refresh: stamps last_update, seeds nothing.
            self.refresh_processes();
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            // 4. Our own start time, for the exclusion's recycled-pid clause.
            self.self_started_at = self
                .system
                .process(Pid::from_u32(self.self_pid))
                .map(|p| p.start_time())
                .unwrap_or(0);
            // 5. Second priming refresh: diffs against zero (a since-boot
            //    average, discarded) and seeds the baseline. The common-path
            //    refresh below is then the first true diff.
            self.refresh_processes();
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            self.primed = true;
        }

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
        let claude = aggregate(&views, &exclusion, self.cpus);

        Sampled {
            stats: SystemStats {
                sampled_at: now_ms,
                mem_total_bytes: self.mem_total_bytes,
                claude,
            },
            elapsed_ms: started.elapsed().as_millis() as u64,
            did_prime,
        }
    }
}

/// Samples every 5 s, publishes into `Core.system`, and fires the presence
/// trigger when Claude Code appears. Never spawns the CLI.
pub async fn run_sampler(
    core: Arc<Core>,
    events: Arc<dyn EventSink>,
    pid_slot: Arc<AtomicU32>,
    shutdown: CancellationToken,
) {
    let mut sampler = Sampler::new(Arc::clone(&pid_slot));
    let mut prev_count: u32 = 0;
    let mut panics: u32 = 0;
    let mut wait = Duration::ZERO;

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = tokio::time::sleep(wait) => {}
        }

        // The sampler is moved into the blocking hop and handed back with
        // the result, so its `System` and its CPU baseline survive across
        // iterations. `let mut owned = sampler;` inside the closure is what
        // makes the `&mut self` call legal on a by-value capture.
        let sampled = match tauri::async_runtime::spawn_blocking(move || {
            let mut owned = sampler;
            let s = owned.sample(chrono::Utc::now().timestamp_millis());
            (owned, s)
        })
        .await
        {
            Ok((next, sampled)) => {
                panics = 0;
                sampler = next;
                sampled
            }
            Err(join) => {
                panics += 1;
                error!(error = %join, attempt = panics, "system sample panicked");
                match after_panic(panics) {
                    None => {
                        error!("system sampler stopped after repeated panics");
                        lock_system(&core.system).stopped = true;
                        events.system_sampled();
                        break;
                    }
                    Some(next_wait) => {
                        sampler = Sampler::new(Arc::clone(&pid_slot));
                        wait = next_wait;
                        continue;
                    }
                }
            }
        };

        // Cancellation can land during the blocking call: publish nothing
        // and wake nobody, so no trigger reaches a driver that is closing.
        if shutdown.is_cancelled() {
            break;
        }

        let Sampled { stats, elapsed_ms, did_prime } = sampled;
        if presence_edge(prev_count, stats.claude.count) {
            info!("presence wake");
            core.triggers.presence();
        }
        if prev_count != stats.claude.count {
            info!(
                count = stats.claude.count,
                rss_bytes = stats.claude.rss_bytes,
                "claude processes changed"
            );
        }
        debug!(
            elapsed_ms,
            count = stats.claude.count,
            rss_bytes = stats.claude.rss_bytes,
            cpu_pct = ?stats.claude.cpu_pct,
            did_prime,
            "system sample"
        );
        prev_count = stats.claude.count;
        lock_system(&core.system).stats = Some(stats);
        events.system_sampled();
        wait = SAMPLE_INTERVAL;
    }
}
```

No imports need adding to the test module: `AtomicU32`, `Arc`, `Duration` and `SAMPLE_INTERVAL` all reach it through its `use super::*`. Adding them again would be an unused import and `-D warnings` would reject it.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml system::tests`
Expected: PASS. The smoke test takes about 400 ms because of the two priming sleeps.

- [ ] **Step 5: Run all four gates**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Expected: PASS. Nothing reads the slot yet, so the sampler runs only once Task 6 spawns it; the gates prove the whole crate still compiles and every existing test is green.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/system.rs src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/src/scheduler/driver.rs
git commit -m "$(cat <<'EOF'
feat(system): add the Claude process sampler and its published slot

Own System, 5 s cadence on the blocking pool. Priming is three refreshes
with two 200 ms gaps because sysinfo seeds no CPU baseline on the first
refresh and diffs against zero on the second. SystemSlot lands here, with
its only writer, and separates "no sample yet" from "the sampler gave up".

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
EOF
)"
```

---

### Task 6: `commands.rs` and `lib.rs` — publish the sample

Spec §4.2. Task 5 published the slot; this task lets the frontend read it and starts the sampler that fills it. `SystemReport` carries `stopped` alongside the stats so the UI never shows a plausible frozen figure without saying so.

**Files:**
- Modify: `src-tauri/src/commands.rs` (`SystemReport`, `core_get_system`, the `get_system` command)
- Modify: `src-tauri/src/lib.rs` (`generate_handler!`, the shared `EventSink`, spawning `run_sampler`)
- Test: `src-tauri/src/commands.rs` (the existing `mod tests`)

**Interfaces:**
- Consumes: `SystemStats`, `ClaudeStats`, `run_sampler`, `SystemSlot`, `lock_system`, `Core.system` (all Task 5); `EventSink::system_sampled` (Task 4).
- Produces:
  - `pub struct SystemReport { pub stats: Option<SystemStats>, pub stopped: bool }`
  - `pub fn core_get_system(core: &Core) -> SystemReport`
  - `#[tauri::command] pub async fn get_system(core: State<'_, SharedCore>) -> AppResult<SystemReport>`
  - the spawned `run_sampler` task in `setup`

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src-tauri/src/commands.rs`:

```rust
    #[test]
    fn core_get_system_reports_no_stats_before_the_first_sample_then_the_sample_then_stopped() {
        let (_tmp, core) = core();

        let report = core_get_system(&core);
        assert_eq!(report.stats, None, "nothing has been sampled yet");
        assert!(!report.stopped);

        let stats = SystemStats {
            sampled_at: 1_700_000_000_000,
            mem_total_bytes: 32 * 1024 * 1024 * 1024,
            claude: ClaudeStats { count: 2, rss_bytes: 1_200_000_000, cpu_pct: Some(3.5) },
        };
        lock_system(&core.system).stats = Some(stats.clone());

        let report = core_get_system(&core);
        assert_eq!(report.stats, Some(stats));
        assert!(!report.stopped);

        lock_system(&core.system).stopped = true;
        let report = core_get_system(&core);
        assert!(report.stopped, "a dead sampler must be distinguishable from a warming one");
        assert!(report.stats.is_some(), "the last sample is kept for the stale rule");
    }
```

Add to the test module's imports:

```rust
    use crate::system::{ClaudeStats, SystemStats};
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml commands::tests::core_get_system`
Expected: FAIL with `cannot find function core_get_system in this scope`.

- [ ] **Step 3: Write the minimal implementation**

In `src-tauri/src/commands.rs`, add the report type and its reader next to `core_get_dashboard`. `SystemSlot` and `lock_system` already exist there from Task 5.

```rust
/// What `get_system` hands the frontend.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemReport {
    pub stats: Option<crate::system::SystemStats>,
    pub stopped: bool,
}

/// Cheap; called on every `system:sampled`. Clones the slot and never
/// touches the store.
pub fn core_get_system(core: &Core) -> SystemReport {
    let slot = lock_system(&core.system);
    SystemReport { stats: slot.stats.clone(), stopped: slot.stopped }
}
```

Add the command wrapper next to `get_dashboard`:

```rust
#[tauri::command]
pub async fn get_system(core: State<'_, SharedCore>) -> AppResult<SystemReport> {
    // Mirrors `get_dashboard`'s shape, but takes no `blocking` hop: this
    // one only clones a mutex-guarded slot and never touches the store.
    let core = Arc::clone(&core);
    Ok(core_get_system(&core))
}
```

In `src-tauri/src/lib.rs`, register the command in `generate_handler!`, after `commands::get_dashboard,`:

```rust
            commands::get_system,
```

And spawn the sampler in `setup`, on the line after `tauri::async_runtime::spawn(driver.run());`. Task 4 already left `events` as an `Arc<dyn EventSink>` it cloned into the driver, and left `pid_slot` unmoved, so this adds a call and nothing else. The sampler shares the driver's `TauriEvents` and its shutdown token:

```rust
            tauri::async_runtime::spawn(system::run_sampler(
                Arc::clone(&core),
                events,
                pid_slot,
                shutdown.clone(),
            ));
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml commands::tests::core_get_system`
Expected: PASS.

- [ ] **Step 5: Run all four gates**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Expected: PASS. The whole Rust side is now complete and the app runs with a live sampler.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(commands): expose the system sample and start the sampler

get_system clones the slot with no store access, and setup spawns
run_sampler beside the driver on the same shutdown token.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
EOF
)"
```

---

### Task 7: `types.ts` and `src/lib/system.ts` — the pure presentation module

Spec §4.4. Every rendering decision lives here, including whether the line is dimmed, so every state is unit-tested and the component stays dumb.

**Files:**
- Modify: `src/lib/types.ts`
- Create: `src/lib/system.ts`
- Test: `src/lib/system.test.ts` (create)

**Interfaces:**
- Consumes: the Rust shapes from Tasks 5 and 6.
- Produces:
  - `export interface ClaudeStats { count: number; rss_bytes: number; cpu_pct: number | null }`
  - `export interface SystemStats { sampled_at: number; mem_total_bytes: number; claude: ClaudeStats }`
  - `export interface SystemReport { stats: SystemStats | null; stopped: boolean }`
  - `export function formatBytes(bytes: number): string`
  - `export function memPct(stats: SystemStats): number | null`
  - `export const SAMPLE_INTERVAL_MS = 5000`
  - `export const STALE_AFTER_MS = 15000`
  - `export function isStale(stats: SystemStats, now: number): boolean`
  - `export function clock(ms: number): string` (zero-padded local HH:MM:SS)
  - `export interface SysItem { key: "cpu" | "mem" | "count" | "none" | "waiting" | "unavailable"; pct: number | null; text: string; title: string }`
  - `export interface SysLine { items: SysItem[]; dimmed: boolean }`
  - `export function systemLine(input: { report: SystemReport | null; error: string | null; showCount: boolean; now: number }): SysLine`
  - `export function processCountSuffix(count: number | null): string`

The state table this implements, from spec §4.4. Rows 1 to 5 are exclusive and evaluated top to bottom; rows 6 to 8 are dim modifiers applied in order to a row 4 or 5 match, the first true one supplying the reason.

| # | condition | items | dimmed | reason appended to every title |
|---|---|---|---|---|
| 1 | `stats === null && error !== null` | "system usage unavailable" | no | the error |
| 2 | `stats === null && stopped` | "system usage unavailable" | no | "sampler stopped, see log" |
| 3 | `stats === null` | "waiting for first sample" | no | none |
| 4 | `count === 0` | "no Claude processes" | rows 6-8 | rows 6-8 |
| 5 | `count > 0` | cpu, mem, count when `showCount` | rows 6-8 | rows 6-8 |
| 6 | `error !== null` | as above | yes | the error |
| 7 | `stopped` | as above | yes | "sampler stopped, see log" |
| 8 | `isStale(stats, now)` | as above | yes | "last sample HH:MM:SS" |

- [ ] **Step 1: Write the failing tests**

Create `src/lib/system.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import {
  clock,
  formatBytes,
  isStale,
  memPct,
  processCountSuffix,
  STALE_AFTER_MS,
  systemLine,
} from "./system";
import type { SystemReport, SystemStats } from "./types";

const GIB = 1024 * 1024 * 1024;

function stats(over: Partial<SystemStats> = {}, claudeOver: Partial<SystemStats["claude"]> = {}): SystemStats {
  return {
    sampled_at: 1_000_000,
    mem_total_bytes: 32 * GIB,
    claude: { count: 2, rss_bytes: Math.round(1.2 * GIB), cpu_pct: 3, ...claudeOver },
    ...over,
  };
}

function report(s: SystemStats | null, stopped = false): SystemReport {
  return { stats: s, stopped };
}

const NOW = 1_000_000;

describe("formatBytes", () => {
  it("uses whole MiB under a GiB and one decimal at or above", () => {
    expect(formatBytes(0)).toBe("0 MB");
    expect(formatBytes(512 * 1024 * 1024)).toBe("512 MB");
    expect(formatBytes(GIB)).toBe("1.0 GB");
    expect(formatBytes(1.25 * GIB)).toBe("1.3 GB");
    expect(formatBytes(20 * GIB)).toBe("20.0 GB");
  });
});

describe("memPct", () => {
  it("is the share of total memory, clamped", () => {
    expect(memPct(stats())).toBeCloseTo(3.75, 2);
    expect(memPct(stats({ mem_total_bytes: 0 }))).toBeNull();
    expect(memPct(stats({ mem_total_bytes: GIB }, { rss_bytes: 4 * GIB }))).toBe(100);
  });
});

describe("isStale", () => {
  it("turns true strictly after the threshold", () => {
    expect(isStale(stats(), NOW + STALE_AFTER_MS)).toBe(false);
    expect(isStale(stats(), NOW + STALE_AFTER_MS + 1)).toBe(true);
  });
});

describe("clock", () => {
  it("zero-pads hours, minutes and seconds", () => {
    expect(clock(new Date(2026, 8, 17, 14, 2, 11).getTime())).toBe("14:02:11");
    expect(clock(new Date(2026, 8, 17, 9, 0, 5).getTime())).toBe("09:00:05");
  });
});

describe("processCountSuffix", () => {
  it("is empty for nothing and singular for one", () => {
    expect(processCountSuffix(null)).toBe("");
    expect(processCountSuffix(0)).toBe("");
    expect(processCountSuffix(1)).toBe(" · 1 Claude process");
    expect(processCountSuffix(2)).toBe(" · 2 Claude processes");
  });
});

describe("systemLine state table", () => {
  it("row 1: no stats with an error is unavailable, undimmed, with the error", () => {
    const line = systemLine({ report: null, error: "boom", showCount: false, now: NOW });
    expect(line.items.map((i) => i.key)).toEqual(["unavailable"]);
    expect(line.items[0].text).toBe("system usage unavailable");
    expect(line.items[0].title).toBe("boom");
    expect(line.dimmed).toBe(false);
  });

  it("row 2: no stats but stopped is unavailable with the sampler reason", () => {
    const line = systemLine({ report: report(null, true), error: null, showCount: false, now: NOW });
    expect(line.items.map((i) => i.key)).toEqual(["unavailable"]);
    expect(line.items[0].title).toBe("sampler stopped, see log");
    expect(line.dimmed).toBe(false);
  });

  it("row 3: no stats and no error is waiting", () => {
    expect(
      systemLine({ report: null, error: null, showCount: false, now: NOW }).items.map((i) => i.key),
    ).toEqual(["waiting"]);
    expect(
      systemLine({ report: report(null), error: null, showCount: false, now: NOW }).items.map((i) => i.key),
    ).toEqual(["waiting"]);
  });

  it("row 4: a zero count is one plain item whatever showCount says", () => {
    for (const showCount of [true, false]) {
      const line = systemLine({
        report: report(stats({}, { count: 0, rss_bytes: 0, cpu_pct: 0 })),
        error: null,
        showCount,
        now: NOW,
      });
      expect(line.items.map((i) => i.key)).toEqual(["none"]);
      expect(line.items[0].text).toBe("no Claude processes");
      expect(line.dimmed).toBe(false);
    }
  });

  it("row 5: a live count is cpu, mem and optionally the count", () => {
    const without = systemLine({ report: report(stats()), error: null, showCount: false, now: NOW });
    expect(without.items.map((i) => i.key)).toEqual(["cpu", "mem"]);
    expect(without.items[0].text).toBe("cpu 3%");
    expect(without.items[0].title).toBe("Claude processes: 3% of the machine's CPU");
    expect(without.items[1].text).toBe("mem 1.2 GB");
    expect(without.items[1].title).toBe("Claude processes: 1.2 GB of 32.0 GB (4%)");

    const with_ = systemLine({ report: report(stats()), error: null, showCount: true, now: NOW });
    expect(with_.items.map((i) => i.key)).toEqual(["cpu", "mem", "count"]);
    expect(with_.items[2].text).toBe("2 procs");
    expect(with_.items[2].title).toBe("Claude Code processes running");
  });

  it("row 5: one process is singular and a null share is a dash", () => {
    const line = systemLine({
      report: report(stats({}, { count: 1, cpu_pct: null })),
      error: null,
      showCount: true,
      now: NOW,
    });
    expect(line.items[0].text).toBe("cpu —");
    expect(line.items[0].pct).toBeNull();
    expect(line.items[2].text).toBe("1 proc");
  });

  it("row 6: an error dims live stats and supplies the reason", () => {
    const line = systemLine({ report: report(stats()), error: "boom", showCount: false, now: NOW });
    expect(line.items.map((i) => i.key)).toEqual(["cpu", "mem"]);
    expect(line.dimmed).toBe(true);
    expect(line.items[0].title.endsWith("boom")).toBe(true);
  });

  it("row 7: stopped dims live stats at once, before they turn stale", () => {
    const line = systemLine({ report: report(stats(), true), error: null, showCount: false, now: NOW });
    expect(line.dimmed).toBe(true);
    expect(line.items[0].title.endsWith("sampler stopped, see log")).toBe(true);
  });

  it("row 8: a stale sample dims and names its time", () => {
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
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/lib/system.test.ts`
Expected: FAIL with `Failed to resolve import "./system"`.

- [ ] **Step 3: Write the minimal implementation**

Add to `src/lib/types.ts`, after the `Dashboard` interface:

```ts
export interface ClaudeStats {
  count: number;
  rss_bytes: number;
  cpu_pct: number | null;
}

export interface SystemStats {
  sampled_at: number;
  mem_total_bytes: number;
  claude: ClaudeStats;
}

export interface SystemReport {
  stats: SystemStats | null;
  stopped: boolean;
}
```

Create `src/lib/system.ts`:

```ts
import type { SystemReport, SystemStats } from "./types";

const MIB = 1024 * 1024;
const GIB = 1024 * MIB;

export const SAMPLE_INTERVAL_MS = 5000;
/** Three missed samples: the sampler has died, or the machine slept. */
export const STALE_AFTER_MS = 3 * SAMPLE_INTERVAL_MS;

const SAMPLER_STOPPED = "sampler stopped, see log";

/** Whole MiB below a GiB, one decimal at or above it. */
export function formatBytes(bytes: number): string {
  if (bytes < GIB) return `${Math.round(bytes / MIB)} MB`;
  return `${(bytes / GIB).toFixed(1)} GB`;
}

/** The Claude processes' share of total memory, or null without a total. */
export function memPct(stats: SystemStats): number | null {
  if (stats.mem_total_bytes === 0) return null;
  const pct = (stats.claude.rss_bytes / stats.mem_total_bytes) * 100;
  return Math.max(0, Math.min(100, pct));
}

export function isStale(stats: SystemStats, now: number): boolean {
  return now - stats.sampled_at > STALE_AFTER_MS;
}

export interface SysItem {
  key: "cpu" | "mem" | "count" | "none" | "waiting" | "unavailable";
  pct: number | null;
  text: string;
  title: string;
}

export interface SysLine {
  items: SysItem[];
  dimmed: boolean;
}

export function processCountSuffix(count: number | null): string {
  if (count === null || count === 0) return "";
  return count === 1 ? " · 1 Claude process" : ` · ${count} Claude processes`;
}

/**
 * HH:MM:SS in the viewer's local zone. Written here rather than reusing
 * `clockOf` from banner.ts, which has no seconds, and not via
 * `toLocaleTimeString`, whose output depends on the host locale and would
 * make the test unpinnable.
 */
export function clock(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number): string => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function countText(count: number): string {
  return count === 1 ? "1 proc" : `${count} procs`;
}

function liveItems(stats: SystemStats, showCount: boolean): SysItem[] {
  const cpu = stats.claude.cpu_pct;
  const mem = memPct(stats);
  const rss = formatBytes(stats.claude.rss_bytes);
  const total = formatBytes(stats.mem_total_bytes);
  const items: SysItem[] = [
    {
      key: "cpu",
      pct: cpu,
      text: cpu === null ? "cpu —" : `cpu ${Math.round(cpu)}%`,
      title:
        cpu === null
          ? "Claude processes: CPU share not yet known"
          : `Claude processes: ${Math.round(cpu)}% of the machine's CPU`,
    },
    {
      key: "mem",
      pct: mem,
      text: `mem ${rss}`,
      title:
        mem === null
          ? `Claude processes: ${rss}`
          : `Claude processes: ${rss} of ${total} (${Math.round(mem)}%)`,
    },
  ];
  if (showCount) {
    items.push({
      key: "count",
      pct: null,
      text: countText(stats.claude.count),
      title: "Claude Code processes running",
    });
  }
  return items;
}

/**
 * The whole rendering decision for the system line, per the spec §4.4 state
 * table. Rows 1 to 5 are exclusive; rows 6 to 8 are dim modifiers applied to
 * a row 4 or 5 match, the first true one supplying the reason.
 */
export function systemLine(input: {
  report: SystemReport | null;
  error: string | null;
  showCount: boolean;
  now: number;
}): SysLine {
  const { report, error, showCount, now } = input;
  const stats = report?.stats ?? null;
  const stopped = report?.stopped ?? false;

  if (stats === null) {
    if (error !== null) {
      return {
        items: [{ key: "unavailable", pct: null, text: "system usage unavailable", title: error }],
        dimmed: false,
      };
    }
    if (stopped) {
      return {
        items: [
          { key: "unavailable", pct: null, text: "system usage unavailable", title: SAMPLER_STOPPED },
        ],
        dimmed: false,
      };
    }
    return {
      items: [
        {
          key: "waiting",
          pct: null,
          text: "waiting for first sample",
          title: "the first figures arrive within a second of launch",
        },
      ],
      dimmed: false,
    };
  }

  const items: SysItem[] =
    stats.claude.count === 0
      ? [
          {
            key: "none",
            pct: null,
            text: "no Claude processes",
            title: "no Claude Code process is running",
          },
        ]
      : liveItems(stats, showCount);

  let reason: string | null = null;
  if (error !== null) reason = error;
  else if (stopped) reason = SAMPLER_STOPPED;
  else if (isStale(stats, now)) reason = `last sample ${clock(stats.sampled_at)}`;

  if (reason === null) return { items, dimmed: false };
  return {
    items: items.map((i) => ({ ...i, title: `${i.title} · ${reason}` })),
    dimmed: true,
  };
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npx vitest run src/lib/system.test.ts`
Expected: PASS, 15 tests: one each for `formatBytes`, `memPct`, `isStale`, `clock` and `processCountSuffix`, and ten in the `systemLine` state-table describe.

- [ ] **Step 5: Run all four gates**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/lib/types.ts src/lib/system.ts src/lib/system.test.ts
git commit -m "$(cat <<'EOF'
feat(ui): add the pure system-line presentation module

Every state of the line, including whether it is dimmed, is decided here and
covered by one test per row of the spec's state table.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
EOF
)"
```

---

### Task 8: `present.ts` — the count on the chip, and where it goes

Spec §4.4. The count is shown exactly once: on the chip when the chip has room and something to say, otherwise on the line. Without `countPlacement` a halted or stalled header at full width would show no count at all.

**Files:**
- Modify: `src/lib/present.ts`
- Test: `src/lib/present.test.ts`

**Interfaces:**
- Consumes: `processCountSuffix` (Task 7).
- Produces:
  - `export function chipFor(dashboard: Dashboard, claudeProcesses: number | null = null): Chip`
  - `export function countPlacement(kind: BannerKind, compact: boolean): "chip" | "line"`

- [ ] **Step 1: Write the failing tests**

Add to `src/lib/present.test.ts`, inside the existing `describe` that covers `chipFor`:

```ts
  it("appends the process count to the active and idle chips only", () => {
    expect(chipFor(base, 2).text).toBe("polling every 60 s · 2 Claude processes");
    expect(chipFor({ ...base, gate: "idle" }, 1).text).toBe("idle · waits for Claude Code · 1 Claude process");
    expect(chipFor({ ...base, halted: "guard" }, 2).text).toBe("polling halted");
    expect(chipFor({ ...base, stalled_at: 1 }, 2).text).toBe("stalled, recovered");
    expect(chipFor({ ...base, binary: { path: null, source: null } }, 2).text).toBe("no claude binary");
    expect(chipFor({ ...base, accounts: [row({ enabled: false })] }, 2).text).toBe("no enabled accounts");
  });

  it("leaves every chip unchanged when the count is unknown", () => {
    expect(chipFor(base, null).text).toBe("polling every 60 s");
    expect(chipFor({ ...base, gate: "idle" }, null).text).toBe("idle · waits for Claude Code");
  });
```

Add a new `describe` block at the end of `src/lib/present.test.ts`:

```ts
describe("countPlacement", () => {
  it("uses the chip only for active and idle at full width", () => {
    expect(countPlacement("active", false)).toBe("chip");
    expect(countPlacement("idle", false)).toBe("chip");
  });

  it("falls back to the line in cards and for every other chip kind", () => {
    expect(countPlacement("active", true)).toBe("line");
    expect(countPlacement("idle", true)).toBe("line");
    for (const kind of ["halted", "stalled", "no_binary", "no_accounts"] as const) {
      expect(countPlacement(kind, false)).toBe("line");
      expect(countPlacement(kind, true)).toBe("line");
    }
  });
});
```

Update the import at the top of `src/lib/present.test.ts`:

```ts
import { accountCountLabel, accountDotColor, chipFor, countPlacement, sessionNote, summarizeModels, weekNote } from "./present";
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/lib/present.test.ts`
Expected: FAIL with `countPlacement is not a function` and a mismatch on the chip text.

- [ ] **Step 3: Write the minimal implementation**

In `src/lib/present.ts`, add the import:

```ts
import { processCountSuffix } from "./system";
```

and, for the `BannerKind` type:

```ts
import type { BannerKind } from "./banner";
```

Replace `chipFor` and add `countPlacement`:

```ts
export function chipFor(dashboard: Dashboard, claudeProcesses: number | null = null): Chip {
  const banner = bannerFor(dashboard);
  switch (banner?.kind) {
    case "halted": return { dot: "crit", text: "polling halted" };
    case "stalled": return { dot: "warn", text: "stalled, recovered" };
    case "no_binary": return { dot: "warn", text: "no claude binary" };
    case "no_accounts": return { dot: "warn", text: "no enabled accounts" };
    // Only these two chips carry the count, so only these two compute it.
    case "active": return {
      dot: "live",
      text: `polling every ${dashboard.interval_secs} s${processCountSuffix(claudeProcesses)}`,
    };
    default: return {
      dot: "idle",
      text: `idle · waits for Claude Code${processCountSuffix(claudeProcesses)}`,
    };
  }
}

/**
 * Where the Claude process count goes, so it is shown exactly once. The chip
 * only carries it when it has room (not cards) and its text is about polling
 * at all; every other chip leaves it to the system line, so a halted or
 * stalled header still says how many Claude processes exist.
 */
export function countPlacement(kind: BannerKind, compact: boolean): "chip" | "line" {
  if (compact) return "line";
  return kind === "active" || kind === "idle" ? "chip" : "line";
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npx vitest run src/lib/present.test.ts`
Expected: PASS, including the pre-existing `maps each banner kind to a dot and short text`, which calls `chipFor` with one argument and relies on the default.

- [ ] **Step 5: Run all four gates**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/lib/present.ts src/lib/present.test.ts
git commit -m "$(cat <<'EOF'
feat(ui): put the Claude process count on the chip, or on the line

countPlacement keeps the count visible exactly once, including when the chip
is busy saying something else.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
EOF
)"
```

---

### Task 9: `gauge.ts` and `Ring` — the small ring

Spec §4.5. A ratio against a limit is a meter, so CPU and memory shares are rings; the count is a plain figure and never a ring. The small variant draws track and arc only and is hidden from assistive technology, because the text beside it is already the accessible name.

**Files:**
- Modify: `src/lib/gauge.ts`
- Modify: `src/components/Ring.tsx`
- Test: `src/lib/gauge.test.ts`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `export const RING_SIZES = { md: RING, sm: { size: 20, stroke: 3, radius: 8.5 } } as const`
  - `RING` stays exported for `AccountCard`
  - `Ring` props: `{ pct: number | null; title?: string } & ({ size?: "md"; label: string } | { size: "sm" })`

- [ ] **Step 1: Write the failing test**

Add to `src/lib/gauge.test.ts`:

```ts
describe("RING_SIZES.sm", () => {
  it("keeps the md geometry and adds a 20px variant", () => {
    expect(RING_SIZES.md).toEqual({ size: 44, stroke: 5, radius: 19.5 });
    expect(RING_SIZES.sm).toEqual({ size: 20, stroke: 3, radius: 8.5 });
  });

  it("dashes the small radius at 0, 50 and 100 percent", () => {
    const r = RING_SIZES.sm.radius;
    const full = 2 * Math.PI * r;
    expect(ringDash(0, r).offset).toBeCloseTo(full, 5);
    expect(ringDash(50, r).offset).toBeCloseTo(full / 2, 5);
    expect(ringDash(100, r).offset).toBeCloseTo(0, 5);
    expect(ringDash(null, r).offset).toBeCloseTo(full, 5);
  });
});
```

Change the import at the top of `src/lib/gauge.test.ts` (currently `import { RING, ringDash } from "./gauge";`) to:

```ts
import { RING, RING_SIZES, ringDash } from "./gauge";
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npx vitest run src/lib/gauge.test.ts`
Expected: FAIL with `RING_SIZES is not defined`.

- [ ] **Step 3: Write the minimal implementation**

In `src/lib/gauge.ts`, add below `RING`:

```ts
/** The two ring sizes: 44px in the cards, 20px on the system line. */
export const RING_SIZES = {
  md: RING,
  sm: { size: 20, stroke: 3, radius: 8.5 },
} as const;
```

Replace `src/components/Ring.tsx`:

```tsx
import type { JSX } from "react";
import { RING_SIZES, ringDash } from "../lib/gauge";
import { metricColor } from "../lib/theme";

type Props = { pct: number | null; title?: string } & (
  | { size?: "md"; label: string }
  | { size: "sm" }
);

/**
 * A single-value meter bent into a circle: track in the grid colour, arc in
 * the same status colour the linear Meter uses, value in text ink (never
 * the series colour), label under. Arc starts at 12 o'clock.
 *
 * The `sm` variant draws track and arc only and is aria-hidden: it sits
 * beside its own label on the system line, which is the accessible name.
 */
export function Ring(props: Props): JSX.Element {
  const { pct, title } = props;
  const small = props.size === "sm";
  const geometry = small ? RING_SIZES.sm : RING_SIZES.md;
  const { circumference, offset } = ringDash(pct, geometry.radius);
  const c = geometry.size / 2;
  const text = pct === null ? "—" : `${Math.round(Math.max(0, Math.min(100, pct)))}%`;

  const arc = (
    <svg
      width={geometry.size}
      height={geometry.size}
      viewBox={`0 0 ${geometry.size} ${geometry.size}`}
      aria-hidden="true"
    >
      <circle cx={c} cy={c} r={geometry.radius} fill="none" stroke="var(--track-off)" strokeWidth={geometry.stroke} />
      {pct !== null && pct > 0 && (
        <circle cx={c} cy={c} r={geometry.radius} fill="none" stroke={metricColor(pct)} strokeWidth={geometry.stroke}
          strokeLinecap="round" strokeDasharray={circumference} strokeDashoffset={offset}
          transform={`rotate(-90 ${c} ${c})`} />
      )}
      {!small && (
        <text x={c} y={c} className="ring-value" textAnchor="middle" dominantBaseline="central">{text}</text>
      )}
    </svg>
  );

  if (small) {
    return <span className="ring-sm" title={title} aria-hidden="true">{arc}</span>;
  }

  return (
    <div className="ring" title={title} role="img" aria-label={`${props.label} ${text}`}>
      {arc}
      <span className="ring-label">{props.label}</span>
    </div>
  );
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npx vitest run src/lib/gauge.test.ts`
Expected: PASS.

- [ ] **Step 5: Run all four gates**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Expected: PASS. `npm run build` proves the four existing `<Ring pct label />` call sites in `AccountCard.tsx` still satisfy the discriminated union.

- [ ] **Step 6: Commit**

```bash
git add src/lib/gauge.ts src/lib/gauge.test.ts src/components/Ring.tsx
git commit -m "$(cat <<'EOF'
feat(ui): add a 20px Ring variant for the system line

Discriminated props so the small ring takes no label, and is aria-hidden
because its neighbouring text is the accessible name.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
EOF
)"
```

---

### Task 10: `useSystem`, `SystemLine`, `Header` and `App` — render the line

Spec §4.4 and §4.5. The hook mirrors `useDashboard`'s sequence guard so a late response never overwrites a newer one, and tears down its subscription on unmount.

**Files:**
- Create: `src/hooks/useSystem.ts`
- Create: `src/components/SystemLine.tsx`
- Modify: `src/components/Header.tsx`
- Modify: `src/App.tsx`
- Modify: `src/styles.css`

**Interfaces:**
- Consumes: `get_system` (Task 6); `systemLine`, `SysLine`, `SysItem` (Task 7); `chipFor`, `countPlacement` (Task 8); `Ring` with `size="sm"` (Task 9).
- Produces:
  - `export function useSystem(): { report: SystemReport | null; error: string | null }`
  - `SystemLine` props: `{ system: SystemReport | null; error: string | null; showCount: boolean; now: number }`
  - `Header` props gain `system: SystemReport | null`, `systemError: string | null`, `now: number`

- [ ] **Step 1: Write the failing check and run it**

There is no unit test for hooks or components in this project: vitest runs in a node environment with no DOM, and `src/lib/*.test.ts` is the only test location. The failing check for this task is therefore the type build. Make it fail by adding the `Header` call site in `src/App.tsx` before anything supports it:

```tsx
        <Header
          dashboard={dashboard}
          settingsOpen={showSettings}
          compact={layout === "cards"}
          system={system}
          systemError={systemError}
          now={now}
          onToggleSettings={() => setShowSettings((v) => !v)}
          onChanged={refetch}
          onError={showError}
        />
```

Run: `npm run build`
Expected: FAIL, naming `system` and `systemError` as unknown identifiers in `src/App.tsx` and `system`, `systemError` and `now` as props that do not exist on `Header`.

- [ ] **Step 2: Write the minimal implementation**

Create `src/hooks/useSystem.ts`:

```ts
import { useCallback, useEffect, useRef, useState } from "react";
import { backend } from "../lib/backend";
import { errorMessage } from "../lib/errors";
import type { SystemReport } from "../lib/types";

interface UseSystem {
  report: SystemReport | null;
  error: string | null;
}

/**
 * The sampler's figures. `system:sampled` is a refetch trigger only: the
 * payload is empty and the whole report is re-read, so a missed event can
 * never leave the line wrong.
 *
 * A successful response always replaces the report, including one whose
 * `stats` is null — that is how the sampler says it has stopped. Only a
 * rejected invoke keeps the previous report, and it sets `error` rather than
 * raising a toast: a failure every 5 s would be a toast storm.
 */
export function useSystem(): UseSystem {
  const [report, setReport] = useState<SystemReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Guards against two in-flight get_system calls resolving out of order:
  // only the call that is still the most recently started one when it
  // resolves may write report/error.
  const seqRef = useRef(0);

  const load = useCallback(async (): Promise<void> => {
    const seq = ++seqRef.current;
    try {
      const next = await backend().invoke<SystemReport>("get_system");
      if (seq === seqRef.current) {
        setReport(next);
        setError(null);
      }
    } catch (e) {
      if (seq === seqRef.current) {
        setError(errorMessage(e));
      }
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | null = null;

    const attach = async (): Promise<void> => {
      try {
        const unlisten = await backend().listen("system:sampled", () => void load());
        if (cancelled) {
          unlisten();
        } else {
          off = unlisten;
        }
      } catch (e) {
        setError(errorMessage(e));
        console.warn("system: could not subscribe", e);
      }
    };

    void attach();
    return () => {
      cancelled = true;
      if (off !== null) off();
    };
  }, [load]);

  return { report, error };
}
```

Create `src/components/SystemLine.tsx`:

```tsx
import type { JSX } from "react";
import { systemLine } from "../lib/system";
import type { SystemReport } from "../lib/types";
import { Ring } from "./Ring";

interface Props {
  system: SystemReport | null;
  error: string | null;
  showCount: boolean;
  now: number;
}

/**
 * The second header line: what the Claude Code processes cost. Every
 * decision — which items, and whether the line is dimmed — is made by the
 * pure `systemLine`; this component only maps the result to markup.
 */
export function SystemLine({ system, error, showCount, now }: Props): JSX.Element {
  const { items, dimmed } = systemLine({ report: system, error, showCount, now });
  return (
    <div
      className={`sysline${dimmed ? " sysline-stale" : ""}`}
      role="group"
      aria-label="Claude process usage"
    >
      {items.map((item) => (
        <span className="sysline-item" key={item.key} title={item.title}>
          {(item.key === "cpu" || item.key === "mem") && <Ring size="sm" pct={item.pct} />}
          <span className="sysline-text">{item.text}</span>
        </span>
      ))}
    </div>
  );
}
```

Replace `src/components/Header.tsx`:

```tsx
import type { JSX } from "react";
import { backend } from "../lib/backend";
import { bannerFor } from "../lib/banner";
import { errorMessage } from "../lib/errors";
import { accountCountLabel, chipFor, countPlacement } from "../lib/present";
import type { Dashboard, SystemReport } from "../lib/types";
import { SystemLine } from "./SystemLine";

interface Props {
  dashboard: Dashboard;
  settingsOpen: boolean;
  compact: boolean;
  system: SystemReport | null;
  systemError: string | null;
  now: number;
  onToggleSettings: () => void;
  onChanged: () => void;
  onError: (message: string) => void;
}

export function Header({
  dashboard,
  settingsOpen,
  compact,
  system,
  systemError,
  now,
  onToggleSettings,
  onChanged,
  onError,
}: Props): JSX.Element {
  const banner = bannerFor(dashboard);
  const count = system?.stats?.claude.count ?? null;
  const placement = countPlacement(banner?.kind ?? "idle", compact);
  const chip = chipFor(dashboard, placement === "chip" ? count : null);

  const run = async (command: string): Promise<void> => {
    try {
      await backend().invoke(command);
      onChanged();
    } catch (e) {
      onError(errorMessage(e));
    }
  };

  return (
    <header className="header">
      <div className="topbar">
        {!compact && (
          <div className="topbar-left">
            <h1 className="topbar-title">Usage Tracker</h1>
            <span className="topbar-count">{accountCountLabel(dashboard.accounts.length)}</span>
          </div>
        )}
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
      <SystemLine
        system={system}
        error={systemError}
        showCount={placement === "line"}
        now={now}
      />
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

In `src/App.tsx`, add the import and the hook call:

```tsx
import { useSystem } from "./hooks/useSystem";
```

```tsx
  const { report: system, error: systemError } = useSystem();
```

Place that line immediately after the `useDashboard` call, so it runs before the `dashboard === null` early return.

Add to `src/styles.css`, after the `.chip-text` rule:

```css
/* ---- system line ---- */
.sysline { display: flex; align-items: center; gap: 14px; flex-wrap: wrap; padding: 6px 0 0; }
.sysline-item { display: flex; align-items: center; gap: 6px; }
.sysline-text { font-family: var(--mono); font-size: 12px; color: #adb6bd; }
.sysline-stale { opacity: 0.55; }
.ring-sm { display: inline-flex; flex: none; }
```

Change the existing `.topbar-actions` rule to wrap:

```css
.topbar-actions { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
```

- [ ] **Step 3: Run the build to verify it passes**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 4: Run all four gates**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/hooks/useSystem.ts src/components/SystemLine.tsx src/components/Header.tsx src/App.tsx src/styles.css
git commit -m "$(cat <<'EOF'
feat(ui): render the Claude process usage line in the header

useSystem mirrors useDashboard's sequence guard and tears its subscription
down on unmount. SystemLine holds no state logic.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
EOF
)"
```

---

### Task 11: "on top" in the header

Spec §5. The toggle leaves Settings for the header, so it is one click away in every layout. Persistence and the apply effect are untouched.

**Files:**
- Modify: `src/components/Header.tsx`
- Modify: `src/App.tsx`
- Modify: `src/components/Settings.tsx:287-292`

**Interfaces:**
- Consumes: the `Header` props from Task 10.
- Produces: `Header` props gain `stayOnTop: boolean` and `onToggleStayOnTop: () => void`.

- [ ] **Step 1: Write the failing check and run it**

Add the new props to the `Header` call site in `src/App.tsx` before the component accepts them:

```tsx
          stayOnTop={prefs.alwaysOnTop}
          onToggleStayOnTop={() => update({ alwaysOnTop: !prefs.alwaysOnTop })}
```

Run: `npm run build`
Expected: FAIL, naming `stayOnTop` and `onToggleStayOnTop` as properties that do not exist on `IntrinsicAttributes & Props`.

- [ ] **Step 2: Write the minimal implementation**

In `src/components/Header.tsx`, add to `interface Props`:

```tsx
  stayOnTop: boolean;
  onToggleStayOnTop: () => void;
```

Add them to the destructured parameter list, between `now` and `onToggleSettings`:

```tsx
  stayOnTop,
  onToggleStayOnTop,
```

Add the button between refresh and settings in `topbar-actions`:

```tsx
          <button
            type="button"
            className={"btn" + (stayOnTop ? " btn-edit-on" : "")}
            aria-pressed={stayOnTop}
            title="Stay on top of other windows"
            onClick={onToggleStayOnTop}
          >
            on top
          </button>
```

In `src/components/Settings.tsx`, delete the whole `Toggle` block for "Keep window on top" (the five lines from `<Toggle` through `/>` that carry `label="Keep window on top"`). Leave the "Debug logging" toggle above it untouched. If `prefs` becomes unused in that file, `npm run build` will say so; it is still used elsewhere in Settings, so no import change is expected.

- [ ] **Step 3: Run the build to verify it passes**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 4: Run all four gates**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Expected: PASS. The `parsePrefs` tests in `src/lib/prefs.test.ts` already cover `alwaysOnTop` and must stay green: persistence has not changed.

- [ ] **Step 5: Commit**

```bash
git add src/components/Header.tsx src/App.tsx src/components/Settings.tsx
git commit -m "$(cat <<'EOF'
feat(ui): move "keep on top" from Settings to a header button

Present in every layout. The apply effect and persistence are unchanged.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
EOF
)"
```

---

### Task 12: `mockBackend` — drive the line in the browser

Spec §4.6. The mock's `listen` is a no-op today, so anything the mock UI must show changing over time has to be driven from the mock itself. The `?mockSystem` flags exist so the dimmed and unavailable states can be screenshotted in Task 14.

**Files:**
- Modify: `src/lib/mockBackend.ts`

**Interfaces:**
- Consumes: `SystemReport`, `SystemStats` (Task 7).
- Produces: a `get_system` handler, a `system:sampled` interval in `listen`, and the `?mockSystem=stopped|error` flags.

- [ ] **Step 1: Write the failing check and run it**

The mock has no unit tests, so its failing check is a manual browser run: `useSystem` already invokes `get_system`, and the mock rejects every command it does not handle.

Run: `VITE_MOCK_BACKEND=1 npm run dev`, then open the printed URL.
Expected: FAIL, in the sense that the system line reads "system usage unavailable" with the tooltip `mock: unknown command get_system`, and the browser console logs the rejected invoke. That is row 1 of the state table answering correctly to a missing handler.

- [ ] **Step 2: Write the minimal implementation**

In `src/lib/mockBackend.ts`, extend the existing `./types` import block (`src/lib/mockBackend.ts:4-12`) rather than adding a second statement. The names stay alphabetical:

```ts
import type {
  Account,
  AppErrorShape,
  Dashboard,
  HistoryPoint,
  RawSnapshot,
  SnapshotDto,
  SystemReport,
  SystemStats,
  UserSettings,
} from "./types";
```

Inside `createMockBackend`, before the `handlers` object, read the flag once:

```ts
  // The only URL-driven behaviour in the mock, so Playwright can reach the
  // dimmed and unavailable states without a rebuild. The real backend never
  // looks at the URL.
  const mockSystem = new URLSearchParams(window.location.search).get("mockSystem");
  const GIB = 1024 * 1024 * 1024;

  const systemStats = (): SystemStats => {
    const drift = 0.95 + Math.random() * 0.1;
    return {
      sampled_at: Date.now(),
      mem_total_bytes: 32 * GIB,
      claude: {
        count: 2,
        rss_bytes: Math.round(1.2 * GIB * drift),
        cpu_pct: Math.round((1 + Math.random() * 7) * 10) / 10,
      },
    };
  };
```

Add the handler to the `handlers` object, next to `get_settings`:

```ts
    get_system: (): SystemReport => {
      if (mockSystem === "error") {
        throw {
          code: "internal",
          message: "mock: sampler unreachable",
        } satisfies AppErrorShape;
      }
      return { stats: systemStats(), stopped: mockSystem === "stopped" };
    },
```

Replace the no-op `listen` in the returned backend:

```ts
    listen: (event: string, handler: () => void) => {
      if (event !== "system:sampled") return Promise.resolve(() => undefined);
      const timer = window.setInterval(handler, 5000);
      return Promise.resolve(() => window.clearInterval(timer));
    },
```

- [ ] **Step 3: Run the mock to verify it passes**

Run: `VITE_MOCK_BACKEND=1 npm run dev`, then open the printed URL.
Expected: the system line reads `cpu N%` and `mem 1.2 GB`, the figures drift every 5 seconds, and the chip reads `polling every 60 s · 2 Claude processes`. Then open the same URL with `?mockSystem=stopped` (the line is dimmed) and with `?mockSystem=error` (the line reads "system usage unavailable").

- [ ] **Step 4: Run all four gates**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/lib/mockBackend.ts
git commit -m "$(cat <<'EOF'
feat(mock): serve get_system and drive system:sampled

sampled_at is stamped per call so the stale rule never trips against the
mock. ?mockSystem=stopped|error reaches the dimmed and unavailable states.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
EOF
)"
```

---

### Task 13: Documentation

Spec §9. The background design doc is the authoritative description of the scheduler, so it must not keep describing the old gate.

**Files:**
- Modify: `docs/2026-09-15-claude-usage-tracker-design.md` (§6.2, §6.5, §8)
- Modify: `docs/superpowers/specs/2026-09-16-history-and-compact-design.md` (§5.5)

**Interfaces:**
- Consumes: the final behaviour from Tasks 1 to 12.
- Produces: no code.

- [ ] **Step 1: Read the sections you are about to change**

Run: `grep -n "^### 6.2\|^### 6.5\|^## 8\|^### 5.5" docs/2026-09-15-claude-usage-tracker-design.md docs/superpowers/specs/2026-09-16-history-and-compact-design.md`
Expected: the four line numbers. Read each section before editing so the rewrite matches the surrounding voice.

- [ ] **Step 2: Rewrite §6.5 rules 4 and 6**

Replace the rule 4 and rule 6 text with the §3.1 wording, and add the decision table verbatim from the spec. Prefix the section with a dated note:

```markdown
> Updated 2026-09-17: the gate now reconciles on every trigger that carries a
> process answer, and a `Presence` trigger wakes the driver when Claude Code
> appears. See `docs/superpowers/specs/2026-09-17-gate-and-system-design.md`.
```

Add `Presence` (`presence`) to the `Trigger` wire-form list and `AlreadyActive` (`already_active`) to the `SkipReason` list. Add the presence arm to the driver-loop sketch, between the startup arm and the account-changed arm:

```
    _ = triggers.notified_presence() => {
        // already active → debug; busy → defer and re-fire at the next
        // flush point; otherwise probe and decide.
    }
```

- [ ] **Step 3: Rewrite §6.2's match rule and add the sampler paragraph**

Replace the description of the gate's exclusion with the `Exclusion` rule:

```markdown
A process counts when `matches_claude(name, cmd)` holds, its pid is not the
live poll child's, and it is not a child of this app started after this app.
The last clause is what keeps a recycled parent pid from hiding a real
session: Windows reports `th32ParentProcessID` even after the parent has
exited. The rule lives in `Exclusion::counts` (`src-tauri/src/process.rs`)
and is shared by the gate and the sampler, so the two can never disagree
about which processes count.
```

Add the sampler paragraph after it:

```markdown
**Sampler.** `src-tauri/src/system.rs` walks the process table every 5 s on
the blocking pool with its own `sysinfo::System`, publishes `SystemStats`
into `Core.system`, emits `system:sampled`, and fires the `Presence` trigger
when the Claude process count goes from zero to non-zero. It never spawns the
CLI. Three consecutive panicking samples stop it for the rest of the run and
set `stopped`, which the header surfaces.
```

- [ ] **Step 4: Add the command and the event to §8**

Add one row or bullet in the command surface, matching the section's existing shape:

```markdown
- `get_system() -> SystemReport { stats: SystemStats | null, stopped: bool }`
  — the sampler's latest figures. No store access.
```

And in the event list:

```markdown
- `system:sampled` — a fresh sample was published; the UI re-reads `get_system`.
```

- [ ] **Step 5: Note the moved toggle**

In `docs/superpowers/specs/2026-09-16-history-and-compact-design.md` §5.5, add one line:

```markdown
> 2026-09-17: the "Keep window on top" toggle moved from Settings to an
> "on top" button in the header, present in every layout.
```

- [ ] **Step 6: Run all four gates and commit**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Expected: PASS (documentation only, but the gates confirm nothing else drifted).

```bash
git add docs/2026-09-15-claude-usage-tracker-design.md docs/superpowers/specs/2026-09-16-history-and-compact-design.md
git commit -m "$(cat <<'EOF'
docs: record the gate reconciliation, the sampler and the moved toggle

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
EOF
)"
```

---

### Task 14: Verification and pull request

Spec §8. The unit tests pin the rules; this task pins what the user actually sees, and proves the gate behaves against a real Claude session.

**Files:**
- No source changes. Screenshots are attached to the PR.

**Interfaces:**
- Consumes: everything from Tasks 1 to 13.
- Produces: a pull request.

- [ ] **Step 1: Take the visual pass against the mock**

Run `VITE_MOCK_BACKEND=1 npm run dev`, then drive the printed URL with the Playwright MCP tools and capture six screenshots:

1. 980 px wide: the full layout with the system line and the count on the chip.
2. 420 px wide: the cards layout with the count as the third item of the line.
3. `?mockSystem=stopped` at 980 px: the line dimmed.
4. `?mockSystem=error` at 980 px: the single "system usage unavailable" item.
5. "on top" pressed and released, then reloaded, showing the pressed state persisted.
6. Settings open, with no "Keep window on top" toggle.

- [ ] **Step 2: Check the 360 px wrap**

Resize to 360 px and confirm the three `topbar-actions` buttons wrap under the chip rather than overflowing, and the system line wraps to at most two lines.

- [ ] **Step 3: Verify against the real backend**

Run: `npm run tauri dev`

Confirm all four behaviours, with a Claude Code session open in another window:

1. The chip shows active on the first cycle, before the cycle finishes.
2. Close Claude, click Refresh: the chip flips to idle, and that Manual poll is the final poll.
3. Open Claude while idle: the chip flips back within about 5 seconds, and a cycle with `trigger=presence` appears in the log.
4. The system line shows a plausible RSS for the open sessions, and the count matches how many Claude processes are running.

Read the log with the tray's "Open log folder" item, or `Get-Content` on the newest file in the app log directory.

- [ ] **Step 4: Run all four gates one last time**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Expected: PASS.

- [ ] **Step 5: Open the pull request**

```bash
git push -u origin gate-and-system
gh pr create --title "Gate reconciliation, Claude process usage, stay-on-top button" --body "$(cat <<'EOF'
Implements `docs/superpowers/specs/2026-09-17-gate-and-system-design.md`.

- The gate reconciles on every trigger that carries a process answer, so the
  chip is right on the first cycle and after every Refresh. Only a trigger
  that polls every enabled account may close the gate, so the final poll is
  never spent on a subset.
- A new sampler publishes the Claude processes' CPU share, resident memory
  and count every 5 s, and wakes the driver when Claude Code appears.
- "Keep window on top" moved from Settings to an "on top" header button.

Screenshots attached: full, cards, dimmed, unavailable, on-top persisted,
Settings without the toggle.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
EOF
)"
```

- [ ] **Step 6: Attach the screenshots and request review**

Attach the six screenshots to the PR body, then follow the `coderabbit-budget` skill's rules before asking for a review.

---

## Self-Review

**1. Spec coverage.** Every section maps to a task: §3.1 to Task 2, §3.2 to Task 4, §3.3 to Task 3, §4.1 to Tasks 1 and 5, §4.2 to Tasks 5 and 6, §4.3 to Tasks 5 and 6, §4.4 to Tasks 7, 8 and 10, §4.5 to Tasks 9 and 10, §4.6 to Task 12, §5 to Task 11, §6 error handling to the tasks that own each path (Tasks 4, 5, 7), §7 logging to Tasks 4 and 5, §8 testing distributed across every task plus Task 14, §9 to Task 13, §10 out of scope to nothing by design.

Every §7 log line maps to a task. To Task 4: `gate changed {gate, trigger}`, `trigger ignored: shutting down {trigger}`, `presence skipped {reason}` for already_active and for `no process answer`, the `decision skipped` line now also covering `already_active`, the WARN `process check failed; gate left unchanged {error}`, and the DEBUG `process check skipped {reason}` with `shutting down` and `busy`. Only `probe_if_free` names why there is no answer; its callers say `no process answer` and leave the reason to the line above, so the two can never drift apart. To Task 5: `presence wake`, `claude processes changed {count, rss_bytes}`, `system sample {elapsed_ms, count, rss_bytes, cpu_pct, did_prime}`, and the two ERROR lines for a panicking sample and a stopped sampler. The `process gate check` DEBUG line is deliberately untouched, per §7: which trigger spent the check is on the line above it.

**2. Placeholder scan.** No "TBD", no "similar to Task N", no "add error handling" without the code. Every code step carries the literal text to write.

**3. Type consistency.** `Exclusion::counts` (Task 1) is called by `aggregate` and `is_claude_running` with the same signature in Tasks 1 and 5. `is_claude_running(sys, &exclusion)` and `ProcessProbe::claude_running(&self, exclude_pid)` keep one signature each across Tasks 1, 4 and 5, so `IdleProcess`, `CountingProcess` and `SysinfoProbe` all implement the same trait method. `Sampled.did_prime` is the per-call flag throughout; `Sampler.primed` is the sticky field and is never logged. `SystemReport` has the same shape in Rust (Task 6) and TypeScript (Task 7). `systemLine` takes one object argument in Tasks 7 and 10. `Ring` is `size="sm"` without a label in Tasks 9 and 10. `Driver::new` has six parameters in Tasks 4 and 6.
