//! Sampler-loop integration tests (spec §4.4, §5.3).
//!
//! Every "no wake" check runs behind a barrier. The sampler publishes
//! (`system_sampled`) and only then reads the memory figure, so the count
//! growing once proves a sample was published, not that its wake check ran.
//! The count growing a second time, after a kick, proves the first
//! iteration, wake check included, is over. A bare sleep-then-assert would
//! pass whether or not the sampler had ever looked at memory.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cut_core::commands::{lock_status, Core};
use cut_core::memory::{floor_bytes, MIB};
use cut_core::scheduler::driver::EventSink;
use cut_core::scheduler::machine::MemoryHold;
use cut_core::store::settings::UserSettings;
use cut_core::system::run_sampler;
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;

mod common;
use common::{defaults, test_core, FakeMemory};

/// Counts `system_sampled`; every other event is ignored.
struct CountingSink {
    samples: AtomicUsize,
}

impl CountingSink {
    fn new() -> Arc<CountingSink> {
        Arc::new(CountingSink { samples: AtomicUsize::new(0) })
    }

    fn count(&self) -> usize {
        self.samples.load(Ordering::SeqCst)
    }
}

impl EventSink for CountingSink {
    fn usage_updated(&self, _account_id: &str) {}
    fn cycle_finished(&self, _peak: Option<cut_core::memory::ChildPeak>) {}
    fn gate_changed(&self, _gate: &str) {}
    fn poller_stalled(&self, _at: i64, _cycle_age_ms: u64) {}
    fn refresh_tray(&self) {}
    fn system_sampled(&self) {
        self.samples.fetch_add(1, Ordering::SeqCst);
    }
    fn memory_hold_changed(&self) {}
    fn settings_applied(&self) {}
}

/// Polls until the sink has counted `at_least` samples, or panics at `limit`.
async fn wait_for_count(sink: &CountingSink, at_least: usize, limit: Duration) {
    let waited = timeout(limit, async {
        while sink.count() < at_least {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(
        waited.is_ok(),
        "sampler never reached {at_least} samples (saw {})",
        sink.count()
    );
}

/// After this returns, at least one full sampler iteration, wake check
/// included, has finished since the sampler started.
async fn barrier(core: &Core, sink: &CountingSink) {
    wait_for_count(sink, 1, Duration::from_secs(10)).await;
    for _ in 0..2 {
        let before = sink.count();
        core.sampler_kick.notify_one();
        wait_for_count(sink, before + 1, Duration::from_secs(10)).await;
    }
}

async fn no_wake(core: &Core) {
    assert!(
        timeout(Duration::from_millis(100), core.triggers.notified_memory_recovered())
            .await
            .is_err(),
        "the sampler woke the driver"
    );
}

fn hold_memory(core: &Core) {
    lock_status(&core.status).memory_hold = Some(MemoryHold {
        available_bytes: 1,
        floor_bytes: floor_bytes(1536),
        since: 0,
    });
}

fn spawn_sampler(
    core: &Arc<Core>,
    sink: &Arc<CountingSink>,
    memory: &Arc<FakeMemory>,
) -> (CancellationToken, tokio::task::JoinHandle<()>) {
    let shutdown = CancellationToken::new();
    let handle = tokio::spawn(run_sampler(
        Arc::clone(core),
        Arc::clone(sink) as Arc<dyn EventSink>,
        Arc::clone(memory) as Arc<dyn cut_core::memory::MemoryProbe>,
        Arc::new(AtomicU32::new(0)),
        shutdown.clone(),
    ));
    (shutdown, handle)
}

async fn stop(shutdown: CancellationToken, handle: tokio::task::JoinHandle<()>) {
    shutdown.cancel();
    let joined = timeout(Duration::from_secs(10), handle).await;
    assert!(matches!(joined, Ok(Ok(()))), "the sampler did not stop cleanly");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_tick_above_the_floor_wakes_the_driver() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (core, _rx) = test_core(dir.path());
    let sink = CountingSink::new();
    let memory = Arc::new(FakeMemory::new());
    hold_memory(&core);
    memory.script(&[Some(u64::MAX)]);

    let (shutdown, handle) = spawn_sampler(&core, &sink, &memory);
    let woke = timeout(Duration::from_secs(10), core.triggers.notified_memory_recovered()).await;
    assert!(woke.is_ok(), "a held tick above the floor must wake the driver");
    stop(shutdown, handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tick_below_the_floor_does_not_wake() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (core, _rx) = test_core(dir.path());
    let sink = CountingSink::new();
    let memory = Arc::new(FakeMemory::new());
    hold_memory(&core);
    memory.script(&[Some(512 * MIB)]);

    let (shutdown, handle) = spawn_sampler(&core, &sink, &memory);
    barrier(&core, &sink).await;
    no_wake(&core).await;
    stop(shutdown, handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_kick_samples_at_once_while_hidden() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (core, _rx) = test_core(dir.path());
    let sink = CountingSink::new();
    let memory = Arc::new(FakeMemory::new());
    core.window_open.store(false, Ordering::SeqCst);

    let (shutdown, handle) = spawn_sampler(&core, &sink, &memory);
    wait_for_count(&sink, 1, Duration::from_secs(10)).await;
    core.sampler_kick.notify_one();
    // Hidden, the next tick is 30 s away: only the kick can explain a
    // second sample inside 3 s.
    wait_for_count(&sink, 2, Duration::from_secs(3)).await;
    stop(shutdown, handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_floor_is_read_from_the_settings_watch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (core, _rx) = test_core(dir.path());
    let sink = CountingSink::new();
    let memory = Arc::new(FakeMemory::new());
    hold_memory(&core);
    memory.script(&[Some(2048 * MIB)]);
    core.settings_tx
        .send(UserSettings { min_free_memory_mb: 4096, ..defaults() })
        .expect("send 4096");

    let (shutdown, handle) = spawn_sampler(&core, &sink, &memory);
    barrier(&core, &sink).await;
    no_wake(&core).await;

    core.settings_tx
        .send(UserSettings { min_free_memory_mb: 1024, ..defaults() })
        .expect("send 1024");
    core.sampler_kick.notify_one();
    let woke = timeout(Duration::from_secs(10), core.triggers.notified_memory_recovered()).await;
    assert!(woke.is_ok(), "lowering the floor under the reading must wake the driver");
    stop(shutdown, handle).await;
}
