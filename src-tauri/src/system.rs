use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::commands::{lock_status, lock_system, Core};
use crate::memory::{floor_bytes, recovery_due, MemoryProbe};
use crate::process::{Exclusion, ProcView};
use crate::scheduler::driver::EventSink;

/// How often the sampler walks the process table while the window is open.
pub const VISIBLE_INTERVAL: Duration = Duration::from_secs(5);

/// How often it walks the table while the window is destroyed (closed to the
/// tray). Presence and memory-recovery latency rise to this.
pub const HIDDEN_INTERVAL: Duration = Duration::from_secs(30);

/// The wait between samples for the window state.
pub fn sample_interval(window_open: bool) -> Duration {
    if window_open {
        VISIBLE_INTERVAL
    } else {
        HIDDEN_INTERVAL
    }
}

/// Three consecutive panicking samples stop the task for the rest of the run.
const MAX_CONSECUTIVE_PANICS: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemStats {
    /// Epoch milliseconds, so the UI can tell a live figure from a frozen one.
    pub sampled_at: i64,
    /// Whole-machine CPU busy share, 0..=100 (100 − PDH "% Idle Time").
    pub cpu_pct: f32,
    /// Whole-machine physical memory in use (total − available), bytes.
    pub mem_used_bytes: u64,
    /// Whole-machine physical memory, bytes; the denominator for the memory ring.
    pub mem_total_bytes: u64,
    /// Claude Code processes other than the poll child and any child of this app.
    pub claude_count: u32,
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

/// The wake condition: Claude Code has just appeared.
pub fn presence_edge(prev_count: u32, next_count: u32) -> bool {
    prev_count == 0 && next_count > 0
}

/// How long to wait after a panicking sample, or `None` to stop for good.
pub fn after_panic(panics: u32, window_open: bool) -> Option<Duration> {
    if panics >= MAX_CONSECUTIVE_PANICS {
        None
    } else {
        Some(sample_interval(window_open))
    }
}

/// What one good sample tells the loop to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SamplerStep {
    /// Wake the driver: a hold exists and the commit headroom is back at or
    /// above the floor. Level-triggered, so there is no edge to lose.
    pub wake: bool,
    pub wait: Duration,
}

pub fn sampler_step(
    held: bool,
    available: Option<u64>,
    floor_bytes: u64,
    window_open: bool,
) -> SamplerStep {
    SamplerStep {
        wake: recovery_due(held, available, floor_bytes),
        wait: sample_interval(window_open),
    }
}

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

/// Samples every 5 s with the window open and every 30 s without, or at once
/// on a `sampler_kick`. Publishes into `Core.system`, fires the presence
/// trigger when Claude Code appears, and wakes the driver on every tick that
/// finds a memory hold with the headroom back above the floor. Never spawns
/// the CLI.
pub async fn run_sampler(
    core: Arc<Core>,
    events: Arc<dyn EventSink>,
    memory: Arc<dyn MemoryProbe>,
    pid_slot: Arc<AtomicU32>,
    shutdown: CancellationToken,
) {
    let mut sampler = Sampler::new(Arc::clone(&pid_slot));
    let mut prev_count: u32 = 0;
    let mut panics: u32 = 0;
    let mut wait = Duration::ZERO;
    let mut last_logged_wait: Option<Duration> = None;

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = core.sampler_kick.notified() => {}
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
                match after_panic(panics, core.window_open.load(Ordering::SeqCst)) {
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
        lock_system(&core.system).stats = Some(stats);
        events.system_sampled();

        let held = lock_status(&core.status).memory_hold.is_some();
        let floor = floor_bytes(core.settings_tx.borrow().min_free_memory_mb);
        let available = memory.available_commit_bytes();
        let step = sampler_step(held, available, floor, core.window_open.load(Ordering::SeqCst));
        if step.wake {
            debug!(available_bytes = available, floor_bytes = floor, "memory recovered; waking driver");
            core.triggers.memory_recovered();
        }
        wait = step.wait;
        if last_logged_wait != Some(wait) {
            debug!(secs = wait.as_secs(), "sample interval");
            last_logged_wait = Some(wait);
        }
    }
}

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
    fn sample_interval_is_5s_open_and_30s_hidden() {
        assert_eq!(VISIBLE_INTERVAL, Duration::from_secs(5));
        assert_eq!(HIDDEN_INTERVAL, Duration::from_secs(30));
        assert_eq!(sample_interval(true), Duration::from_secs(5));
        assert_eq!(sample_interval(false), Duration::from_secs(30));
    }

    #[test]
    fn sampler_step_wakes_only_while_held_and_at_or_above_the_floor() {
        const FLOOR: u64 = 1_610_612_736;
        for held in [true, false] {
            for available in [None, Some(FLOOR - 1), Some(FLOOR), Some(FLOOR + 1)] {
                let expected = held && matches!(available, Some(a) if a >= FLOOR);
                assert_eq!(
                    sampler_step(held, available, FLOOR, true).wake,
                    expected,
                    "held={held} available={available:?}"
                );
            }
        }
    }

    #[test]
    fn sampler_step_waits_by_window_state() {
        assert_eq!(sampler_step(false, None, 0, true).wait, Duration::from_secs(5));
        assert_eq!(sampler_step(false, None, 0, false).wait, Duration::from_secs(30));
        assert_eq!(sampler_step(true, Some(u64::MAX), 0, true).wait, Duration::from_secs(5));
        assert_eq!(sampler_step(true, Some(u64::MAX), 0, false).wait, Duration::from_secs(30));
    }

    #[test]
    fn after_panic_waits_one_interval_for_the_window_state_then_gives_up_on_the_third() {
        for panics in [1, 2] {
            assert_eq!(after_panic(panics, true), Some(Duration::from_secs(5)));
            assert_eq!(after_panic(panics, false), Some(Duration::from_secs(30)));
        }
        assert_eq!(after_panic(3, true), None, "three in a row stops the sampler");
        assert_eq!(after_panic(3, false), None);
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
