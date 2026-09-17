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
