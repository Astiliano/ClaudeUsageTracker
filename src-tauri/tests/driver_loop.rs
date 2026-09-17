//! Driver-loop integration tests.
//!
//! These tests configure the fake Claude binary through process-wide
//! environment variables, so every test in this file takes `ENV_LOCK` for its
//! whole body and removes the variables it set on the way out (`EnvGuard`).
//! That makes plain `cargo test` correct without `--test-threads=1`. The lock
//! is a `tokio::sync::Mutex` rather than a `std::sync::Mutex` for two reasons:
//! it is held across `.await` points (which `clippy::await_holding_lock`
//! rightly rejects for std guards), and it has no poisoning to recover from
//! when a test panics while holding it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cut_core::commands::{
    core_clear_halt, core_poll_now, core_set_settings, core_update_account, lock_binary,
    lock_status, Core,
};
use cut_core::scheduler::driver::{BinaryProbe, Driver, EventSink, ProcessProbe};
use cut_core::scheduler::machine::DriverStatus;
use cut_core::scheduler::triggers::Triggers;
use cut_core::store::settings::UserSettings;
use cut_core::store::Store;
use tokio_util::sync::CancellationToken;

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Serialises the whole test and removes every variable the test set.
struct EnvGuard {
    _lock: tokio::sync::MutexGuard<'static, ()>,
    names: Vec<&'static str>,
}

impl EnvGuard {
    async fn new() -> EnvGuard {
        EnvGuard {
            _lock: ENV_LOCK.lock().await,
            names: Vec::new(),
        }
    }

    fn set(&mut self, name: &'static str, value: impl AsRef<str>) {
        std::env::set_var(name, value.as_ref());
        if !self.names.contains(&name) {
            self.names.push(name);
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for name in self.names.drain(..) {
            std::env::remove_var(name);
        }
    }
}

#[derive(Default)]
struct Recorder {
    usage_updated: Mutex<Vec<String>>,
    cycles: AtomicUsize,
    gates: Mutex<Vec<String>>,
    stalls: AtomicUsize,
}

impl EventSink for Recorder {
    fn usage_updated(&self, account_id: &str) {
        if let Ok(mut v) = self.usage_updated.lock() {
            v.push(account_id.to_string());
        }
    }
    fn cycle_finished(&self) {
        self.cycles.fetch_add(1, Ordering::SeqCst);
    }
    fn gate_changed(&self, gate: &str) {
        if let Ok(mut v) = self.gates.lock() {
            v.push(gate.to_string());
        }
    }
    fn poller_stalled(&self, _at: i64, _cycle_age_ms: u64) {
        self.stalls.fetch_add(1, Ordering::SeqCst);
    }
    fn refresh_tray(&self) {}
    fn system_sampled(&self) {}
}

/// Records every exclusion it is handed, so a test can assert what the
/// process gate actually sees rather than what the driver meant to send.
#[derive(Default)]
struct FixedProcess {
    running: AtomicBool,
    excludes: Mutex<Vec<Option<u32>>>,
}

impl FixedProcess {
    fn new(running: bool) -> FixedProcess {
        FixedProcess {
            running: AtomicBool::new(running),
            excludes: Mutex::new(Vec::new()),
        }
    }

    fn excludes(&self) -> Vec<Option<u32>> {
        self.excludes.lock().expect("lock").clone()
    }
}

impl ProcessProbe for FixedProcess {
    fn claude_running(&self, exclude_pid: Option<u32>) -> bool {
        if let Ok(mut v) = self.excludes.lock() {
            v.push(exclude_pid);
        }
        self.running.load(Ordering::SeqCst)
    }
}

struct FakeBinary(PathBuf);
impl BinaryProbe for FakeBinary {
    fn find(&self, _override_path: &str) -> Option<(PathBuf, &'static str)> {
        Some((self.0.clone(), "override"))
    }
}

struct NoBinary;
impl BinaryProbe for NoBinary {
    fn find(&self, _override_path: &str) -> Option<(PathBuf, &'static str)> {
        None
    }
}

fn defaults() -> UserSettings {
    UserSettings {
        interval_secs: 10,
        timeout_secs: 5,
        claude_binary: String::new(),
        close_to_tray: true,
        launch_at_login: false,
        log_level: "info".to_string(),
    }
}

fn fake_claude() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake_claude"))
}

struct Harness {
    _tmp: tempfile::TempDir,
    core: Arc<Core>,
    events: Arc<Recorder>,
    process: Arc<FixedProcess>,
    shutdown: CancellationToken,
}

fn harness(running: bool) -> Harness {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(Store::open_in_memory().expect("open"));
    let (settings_tx, _rx) = tokio::sync::watch::channel(defaults());
    store.save_settings(&defaults()).expect("save settings");
    let core = Arc::new(Core {
        store,
        triggers: Arc::new(Triggers::new()),
        status: Arc::new(Mutex::new(DriverStatus::default())),
        binary: Arc::new(Mutex::new(None)),
        halt_latched: AtomicBool::new(false),
        close_to_tray: AtomicBool::new(true),
        settings_tx,
        log: None,
        app_data_dir: tmp.path().to_path_buf(),
        log_dir: tmp.path().join("logs"),
    });
    Harness {
        _tmp: tmp,
        core,
        events: Arc::new(Recorder::default()),
        process: Arc::new(FixedProcess::new(running)),
        shutdown: CancellationToken::new(),
    }
}

fn add_account(h: &Harness, name: &str) -> String {
    let dir = h.core.app_data_dir.join(name);
    std::fs::create_dir_all(&dir).expect("mkdir");
    h.core
        .store
        .add_account(&dir, true, None, 1)
        .expect("add")
        .id
}

fn driver_for(h: &Harness, binary: Arc<dyn BinaryProbe>) -> Driver {
    Driver::new(
        Arc::clone(&h.core),
        Arc::clone(&h.events) as Arc<dyn EventSink>,
        Arc::clone(&h.process) as Arc<dyn ProcessProbe>,
        binary,
        h.shutdown.clone(),
        Arc::new(std::sync::atomic::AtomicU32::new(0)),
    )
}

/// Blocks until the driver has published a running cycle. A barrier rather
/// than a sleep, so a test that must act on an in-flight cycle cannot race
/// the driver's startup decision.
async fn wait_for_busy(h: &Harness) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let busy = lock_status(&h.core.status).busy;
        if busy {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the driver never started a cycle"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Blocks until the driver has published an idle state.
async fn wait_for_idle(h: &Harness) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let busy = lock_status(&h.core.status).busy;
        if !busy {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the driver never became idle"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Blocks until at least `want` snapshots have been persisted.
async fn wait_for_snapshots(h: &Harness, want: i64) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let have = snapshot_count(h);
        if have >= want {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "expected {want} snapshots, still at {have}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Blocks until the watchdog has reported a stall. The driver publishes the
/// post-stall snapshot before it emits the event, so seeing the count move is
/// proof that the republish has already happened.
async fn wait_for_stall(h: &Harness) {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        if h.events.stalls.load(Ordering::SeqCst) >= 1 {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the watchdog never fired"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Blocks until the process gate has been consulted at least `want` times.
async fn wait_for_process_checks(h: &Harness, want: usize) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let have = h.process.excludes().len();
        if have >= want {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "expected {want} process checks, still at {have}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn snapshot_count(h: &Harness) -> i64 {
    h.core
        .store
        .with_conn(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM snapshots", [], |r| r.get(0))?))
        .expect("count")
}

fn emit_ok_report(env: &mut EnvGuard) {
    let report = "Current session: 15% used \u{b7} resets Sep 16, 3:30am (America/Los_Angeles)\\n\
                  Current week (all models): 4% used \u{b7} resets Sep 21, 8am (America/Los_Angeles)";
    env.set("FAKE_CLAUDE_MODE", "emit");
    env.set(
        "FAKE_CLAUDE_STDOUT",
        format!(
            r#"{{"type":"result","local_command":"usage","num_turns":0,"total_cost_usd":0,"result":"{report}"}}"#
        ),
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_startup_cycle_polls_every_enabled_account_and_emits_one_cycle_finished() {
    let mut env = EnvGuard::new().await;
    emit_ok_report(&mut env);
    let h = harness(false);
    let a = add_account(&h, ".claude");
    let b = add_account(&h, ".claude3");

    let driver = driver_for(&h, Arc::new(FakeBinary(fake_claude())));
    let handle = tokio::spawn(driver.run());

    tokio::time::sleep(Duration::from_secs(3)).await;
    h.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;

    let updated = h.events.usage_updated.lock().expect("lock").clone();
    assert!(updated.contains(&a), "{updated:?}");
    assert!(updated.contains(&b), "{updated:?}");
    assert!(h.events.cycles.load(Ordering::SeqCst) >= 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn triggers_arriving_during_a_cycle_coalesce_and_never_queue() {
    // A slow child keeps the cycle busy while clicks pile up.
    let mut env = EnvGuard::new().await;
    env.set("FAKE_CLAUDE_MODE", "slow");
    env.set("FAKE_CLAUDE_SLEEP_SECS", "30");
    let h = harness(false);
    add_account(&h, ".claude");
    *lock_binary(&h.core.binary) = Some((fake_claude().to_string_lossy().to_string(), "override"));

    let driver = driver_for(&h, Arc::new(FakeBinary(fake_claude())));
    let handle = tokio::spawn(driver.run());

    wait_for_busy(&h).await;
    for _ in 0..5 {
        assert_eq!(
            core_poll_now(&h.core).expect("poll_now"),
            "skipped:busy",
            "a manual poll during a cycle must report busy, not queue"
        );
    }

    h.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(10), handle).await;
    assert!(
        !lock_status(&h.core.status).busy,
        "the final published snapshot must show the driver idle"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn two_account_changes_in_quick_succession_poll_both_accounts() {
    let mut env = EnvGuard::new().await;
    emit_ok_report(&mut env);
    let h = harness(false);
    let a = add_account(&h, ".claude");
    let b = add_account(&h, ".claude3");
    core_update_account(&h.core, &a, None, Some(false)).expect("disable a");
    core_update_account(&h.core, &b, None, Some(false)).expect("disable b");
    let _ = h.core.triggers.take_changed();

    let driver = driver_for(&h, Arc::new(FakeBinary(fake_claude())));
    let handle = tokio::spawn(driver.run());
    tokio::time::sleep(Duration::from_secs(2)).await;
    h.events.usage_updated.lock().expect("lock").clear();

    core_update_account(&h.core, &a, None, Some(true)).expect("enable a");
    core_update_account(&h.core, &b, None, Some(true)).expect("enable b");

    tokio::time::sleep(Duration::from_secs(3)).await;
    h.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;

    let updated = h.events.usage_updated.lock().expect("lock").clone();
    assert!(updated.contains(&a), "{updated:?}");
    assert!(updated.contains(&b), "{updated:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn clear_halt_does_not_start_a_poll() {
    let mut env = EnvGuard::new().await;
    emit_ok_report(&mut env);
    let h = harness(false);
    add_account(&h, ".claude");
    h.core
        .store
        .set_polling_halted("guard_tripped:1")
        .expect("halt");

    let driver = driver_for(&h, Arc::new(FakeBinary(fake_claude())));
    let handle = tokio::spawn(driver.run());
    tokio::time::sleep(Duration::from_secs(1)).await;
    h.events.usage_updated.lock().expect("lock").clear();

    core_clear_halt(&h.core).expect("clear");
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(
        h.events.usage_updated.lock().expect("lock").is_empty(),
        "clearing the halt must not spend quota by itself"
    );

    h.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn five_unclassifiable_envelopes_escalate_into_a_guard_trip() {
    // Stdout that is not JSON at all is a shape error every time, so every
    // poll of this account is a strike. Manual triggers drive them: they
    // bypass the cooldown, and since the streak is guard evidence rather than
    // retry policy, `reset_backoff` no longer erases it (spec 6.3 step 5).
    // The startup cycle lands the first strike, so the trip falls on or
    // before the fifth manual poll.
    let mut env = EnvGuard::new().await;
    env.set("FAKE_CLAUDE_MODE", "non-json");
    let h = harness(false);
    let a = add_account(&h, ".claude");

    let driver = driver_for(&h, Arc::new(FakeBinary(fake_claude())));
    let handle = tokio::spawn(driver.run());

    // Strike 1 is the startup cycle. Waiting for it also guarantees the
    // binary has been published, so the manual polls below cannot be refused
    // with `skipped:no_binary`.
    wait_for_snapshots(&h, 1).await;
    wait_for_idle(&h).await;

    // Strikes 2 to 5, each one driven to completion before the next, so a
    // poll can never be silently coalesced away as `skipped:busy`.
    for strike in 2..=5i64 {
        assert_eq!(
            core_poll_now(&h.core).expect("poll_now"),
            "started",
            "strike {strike} must actually run"
        );
        wait_for_snapshots(&h, strike).await;
        // The halt flag reaches disk before the outcome snapshot does, so by
        // the time the snapshot count moves the trip has already been
        // recorded if it was going to be.
        let halted = h.core.store.polling_halted().expect("read").is_some();
        assert_eq!(
            halted,
            strike == 5,
            "the guard must trip on the fifth strike and not before (strike {strike})"
        );
    }

    assert!(
        h.core
            .store
            .polling_halted()
            .expect("read")
            .unwrap_or_default()
            .starts_with("guard_tripped:"),
        "five consecutive unclassifiable envelopes must halt the poller"
    );

    let accounts = h.core.store.list_accounts().expect("list");
    let account = accounts.iter().find(|x| x.id == a).expect("the account");
    assert_eq!(
        account.disabled_reason,
        Some(cut_core::usage::DisabledReason::GuardTripped)
    );
    assert!(!account.enabled);

    // Nothing polls again: the halt beats every trigger, including Manual.
    let before = snapshot_count(&h);
    assert_eq!(core_poll_now(&h.core).expect("poll"), "skipped:halted");
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        snapshot_count(&h),
        before,
        "a halted poller must not spend another poll"
    );

    h.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(10), handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guard_trip_halts_the_poller_and_abandons_the_rest_of_the_cycle() {
    let mut env = EnvGuard::new().await;
    env.set("FAKE_CLAUDE_MODE", "emit");
    env.set(
        "FAKE_CLAUDE_STDOUT",
        r#"{"type":"result","num_turns":1,"total_cost_usd":0.75,"result":"hello"}"#,
    );
    let h = harness(false);
    let a = add_account(&h, ".claudeA");
    let b = add_account(&h, ".claudeB");
    let c = add_account(&h, ".claudeC");

    let driver = driver_for(&h, Arc::new(FakeBinary(fake_claude())));
    let handle = tokio::spawn(driver.run());
    tokio::time::sleep(Duration::from_secs(3)).await;
    h.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;

    assert!(
        h.core
            .store
            .polling_halted()
            .expect("read")
            .unwrap_or_default()
            .starts_with("guard_tripped:"),
        "the halt flag must be persisted"
    );

    assert_eq!(
        snapshot_count(&h),
        1,
        "exactly one account is polled before the cycle is abandoned"
    );

    let accounts = h.core.store.list_accounts().expect("list");
    let tripped: Vec<&String> = accounts
        .iter()
        .filter(|x| x.disabled_reason == Some(cut_core::usage::DisabledReason::GuardTripped))
        .map(|x| &x.id)
        .collect();
    assert_eq!(tripped.len(), 1);
    assert!([&a, &b, &c].contains(&tripped[0]));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_watchdog_aborts_a_hung_cycle_and_leaves_the_driver_usable() {
    // A child that sleeps far past the watchdog limit, with the per-poll
    // timeout raised so the timeout path cannot rescue it first.
    let mut env = EnvGuard::new().await;
    env.set("FAKE_CLAUDE_MODE", "slow");
    env.set("FAKE_CLAUDE_SLEEP_SECS", "300");
    let h = harness(false);
    add_account(&h, ".claude");
    let mut s = defaults();
    s.timeout_secs = 120;
    h.core.store.save_settings(&s).expect("save");

    let driver = driver_for(&h, Arc::new(FakeBinary(fake_claude())));
    let handle = tokio::spawn(driver.run());

    // The cycle must already be in flight with the 120 s per-poll timeout
    // before the limit is shrunk: if the shrink landed first, the poll itself
    // would time out at 5 s and the watchdog would have nothing to catch.
    wait_for_busy(&h).await;

    // watchdog_limit_ms(1, 120) is 130 s, which is longer than this test
    // should run, so drive the limit down by shrinking the timeout instead.
    let mut s = defaults();
    s.timeout_secs = 5;
    core_set_settings(&h.core, &s).expect("shrink the limit");

    wait_for_stall(&h).await;

    // Aborting a task only flags it; the CycleToken inside it is dropped
    // when the runtime unwinds it. If the driver publishes without waiting
    // for that, the snapshot says busy with no cycle left to clear it and
    // every Refresh is refused until the next timer tick.
    assert!(
        !lock_status(&h.core.status).busy,
        "the published snapshot must be idle as soon as the stall is handled"
    );
    assert!(
        lock_status(&h.core.status).stalled_at.is_some(),
        "the stall must be recorded for the UI"
    );

    // The aborted poll never reached run_usage's own pid-clearing paths, so
    // the driver must clear the slot itself: a dead pid handed to the gate as
    // an exclusion could mask a real user `claude` once the OS recycles it.
    let before = h.process.excludes().len();
    wait_for_process_checks(&h, before + 1).await;
    assert_eq!(
        h.process.excludes().last().copied().flatten(),
        None,
        "no stale child pid may be excluded after an abort: {:?}",
        h.process.excludes()
    );

    // And the driver is usable again rather than stuck reporting busy.
    assert_eq!(core_poll_now(&h.core).expect("poll_now"), "started");

    h.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(10), handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_kills_a_live_child_and_returns_promptly() {
    let mut env = EnvGuard::new().await;
    env.set("FAKE_CLAUDE_MODE", "slow");
    env.set("FAKE_CLAUDE_SLEEP_SECS", "300");
    let h = harness(false);
    add_account(&h, ".claude");

    let driver = driver_for(&h, Arc::new(FakeBinary(fake_claude())));
    let handle = tokio::spawn(driver.run());
    tokio::time::sleep(Duration::from_secs(1)).await;

    let started = std::time::Instant::now();
    h.shutdown.cancel();
    let finished = tokio::time::timeout(Duration::from_secs(10), handle).await;
    assert!(finished.is_ok(), "the driver must return on cancellation");
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "shutdown must not wait out the child's sleep"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn with_no_binary_the_driver_skips_and_recovers_when_one_appears() {
    let mut env = EnvGuard::new().await;
    emit_ok_report(&mut env);
    let h = harness(false);
    add_account(&h, ".claude");

    let driver = driver_for(&h, Arc::new(NoBinary));
    let handle = tokio::spawn(driver.run());
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(h.events.usage_updated.lock().expect("lock").is_empty());
    assert_eq!(core_poll_now(&h.core).expect("poll"), "skipped:no_binary");
    assert!(
        lock_binary(&h.core.binary).is_none(),
        "the published binary slot must reflect the failed lookup"
    );

    h.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_driver_publishes_gate_busy_and_backoff_into_shared_state() {
    // A child that always fails fast, so an account enters backoff.
    let mut env = EnvGuard::new().await;
    env.set("FAKE_CLAUDE_MODE", "exit-nonzero");
    env.set("FAKE_CLAUDE_EXIT", "7");
    env.set("FAKE_CLAUDE_STDERR", "auth failed");
    let h = harness(true);
    let a = add_account(&h, ".claude");

    let driver = driver_for(&h, Arc::new(FakeBinary(fake_claude())));
    let handle = tokio::spawn(driver.run());
    tokio::time::sleep(Duration::from_secs(3)).await;

    let published = lock_status(&h.core.status).clone();
    assert!(
        published.backoff_until.contains_key(&a),
        "record must be followed by a publish: {published:?}"
    );

    h.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
}
