use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::commands::{blocking, lock_binary, lock_status, Core};
use crate::error::AppResult;
use crate::scheduler::machine::{
    begin_cycle, lock_machine, CycleToken, Decision, DriverStatus, Gate, Machine, Recorded,
    SharedMachine, Trigger,
};
use crate::store::settings::UserSettings;
use crate::usage::runner::run_usage;
use crate::usage::PollOutcome;

/// D10: prune at startup and every 24 h.
pub const PRUNE_INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;

/// The watchdog arm ticks this often while a cycle is in flight.
const WATCHDOG_TICK: Duration = Duration::from_secs(5);

/// The driver's outbound events. Abstracted so the loop is testable without a
/// Tauri runtime. Events are refetch triggers only: the frontend ignores the
/// payloads and re-reads state through commands.
pub trait EventSink: Send + Sync {
    fn usage_updated(&self, account_id: &str);
    fn cycle_finished(&self);
    fn gate_changed(&self, gate: &str);
    fn poller_stalled(&self, at: i64, cycle_age_ms: u64);
    fn refresh_tray(&self);
    /// The sampler published a fresh `SystemStats` (spec §4.2).
    fn system_sampled(&self);
}

/// The process gate, abstracted for the same reason.
pub trait ProcessProbe: Send + Sync {
    fn claude_running(&self, exclude_pid: Option<u32>) -> bool;
}

/// Binary presence is re-checked before every decision, so a first-run
/// "binary not found" state clears as soon as the user fixes Settings.
pub trait BinaryProbe: Send + Sync {
    fn find(&self, override_path: &str) -> Option<(PathBuf, &'static str)>;
}

pub struct RealBinaryProbe;

impl BinaryProbe for RealBinaryProbe {
    fn find(&self, override_path: &str) -> Option<(PathBuf, &'static str)> {
        crate::discovery::find_claude_binary(Some(override_path))
            .map(|f| (f.path, f.source.as_str()))
    }
}

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

/// D5: the interval is the gap between the end of one cycle and the start of
/// the next, so the deadline is always measured from the last cycle's end. A
/// settings change recomputes it from the same anchor instead of restarting
/// the clock.
pub fn deadline_for(last_cycle_end_ms: i64, interval_secs: u32) -> i64 {
    last_cycle_end_ms + i64::from(interval_secs) * 1000
}

/// Spec 6.5: `enabled x timeout_secs + 10 s`.
pub fn watchdog_limit_ms(enabled_accounts: usize, timeout_secs: u32) -> u64 {
    let per_account = enabled_accounts as u64 * u64::from(timeout_secs) * 1000;
    per_account + 10_000
}

pub fn halt_value(now_ms: i64) -> String {
    format!("guard_tripped:{now_ms}")
}

/// Spec 5.1: the driver is the only writer of `DriverStatus`, and it writes
/// one after every `decide()` and every `record()`. `stalled_at` belongs to
/// the watchdog rather than to `Machine`, so it is carried across from the
/// snapshot already in the slot.
pub fn publish_status(machine: &SharedMachine, slot: &Mutex<DriverStatus>, now: i64) {
    let mut fresh = lock_machine(machine).status(now);
    let mut current = lock_status(slot);
    fresh.stalled_at = current.stalled_at;
    *current = fresh;
}

/// The four steps of a guard trip, in the order spec 6.3 mandates.
pub trait HaltSink {
    fn persist_halt(&self, value: &str) -> AppResult<()>;
    fn log_envelope(&self, raw: &str, reason: &str);
    fn persist_outcome(&self) -> AppResult<()>;
    fn disable_account(&self) -> AppResult<()>;
}

/// The halt flag is the safety property, so it reaches disk first. If that
/// write fails nothing else runs and the caller still aborts the cycle.
pub fn perform_halt<S: HaltSink>(sink: &S, now_ms: i64, raw: &str, reason: &str) -> AppResult<()> {
    sink.persist_halt(&halt_value(now_ms))?;
    sink.log_envelope(raw, reason);
    sink.persist_outcome()?;
    sink.disable_account()?;
    Ok(())
}

/// Re-arms a deferred `AccountChanged` notification. The ids themselves are
/// still accumulated inside `Triggers`; only the notification permit was
/// consumed, so an empty `account_changed` is what carries them into the next
/// decision. Called at every point where the reason for deferring has just
/// gone away: a cycle ending normally, a cycle reaped by the watchdog, and a
/// cycle reaped by the backstop.
fn flush_deferred_changes(core: &Core, deferred: &mut bool) {
    if *deferred {
        *deferred = false;
        core.triggers.account_changed(Vec::new());
    }
}

/// Re-arms a presence wake that was consumed while a cycle was running.
/// The wake is edge-triggered and the process count stays non-zero
/// afterwards, so without this the gate would wait for the next Timer.
fn flush_deferred_presence(core: &Core, deferred: &mut bool) {
    if *deferred {
        *deferred = false;
        core.triggers.presence();
    }
}

/// Arms the in-memory halt latch and then runs the four halt steps.
///
/// The latch is raised *before* the `blocking` hop that persists the flag,
/// because the persist can fail: without it `Driver::halted` would re-read
/// `polling_halted`, get `None`, and poll the very account whose envelope
/// just tripped the quota guard — with `disable_account` never having run
/// either. A store write that fails is logged and the cycle is abandoned
/// exactly as before; the difference is only that polling stays closed.
async fn arm_and_perform_halt<S>(core: &Arc<Core>, sink: S, now_ms: i64, raw: String, reason: String)
where
    S: HaltSink + Send + 'static,
{
    core.halt_latched.store(true, Ordering::SeqCst);
    if let Err(e) = blocking(move || perform_halt(&sink, now_ms, &raw, &reason)).await {
        error!(error = %e, "could not fully record the guard trip");
    }
}

/// Bundle passed into a cycle task, so the task owns everything it needs.
struct CycleInputs {
    core: Arc<Core>,
    events: Arc<dyn EventSink>,
    machine: SharedMachine,
    accounts: Vec<String>,
    trigger: Trigger,
    binary: PathBuf,
    cwd: PathBuf,
    timeout: Duration,
    pid_slot: Arc<AtomicU32>,
    cancel: CancellationToken,
}

/// Adapter that performs the four halt steps against the real store. Owns
/// its data rather than borrowing, because the whole sequence runs inside one
/// `blocking` hop and must be `Send + 'static`.
struct StoreHalt {
    core: Arc<Core>,
    account_id: String,
    outcome: PollOutcome,
    raw: Option<String>,
    taken_at: i64,
    duration_ms: u32,
}

impl HaltSink for StoreHalt {
    fn persist_halt(&self, value: &str) -> AppResult<()> {
        self.core.store.set_polling_halted(value)
    }
    /// The only place a guard trip is logged. `run_usage` stays silent so
    /// this ERROR line can never appear before the halt flag reaches disk.
    fn log_envelope(&self, raw: &str, reason: &str) {
        error!(
            account_id = %self.account_id,
            envelope = raw,
            reason = reason,
            "guard tripped: polling halted"
        );
    }
    fn persist_outcome(&self) -> AppResult<()> {
        self.core
            .store
            .insert_snapshot(
                &self.account_id,
                self.taken_at,
                &self.outcome,
                self.raw.as_deref(),
                self.duration_ms,
            )
            .map(|_| ())
    }
    fn disable_account(&self) -> AppResult<()> {
        self.core.store.mark_guard_tripped(&self.account_id)
    }
}

/// Spec 6.3: what a finished poll means for the guard, given the outcome and
/// what `Machine::record` made of it. Two different faults reach the same
/// halt sequence: an envelope that trips the guard outright, and a fifth
/// consecutive unclassifiable envelope on one account, which `record`
/// reports as `Escalate` and which is stored as a guard trip in its own
/// right. Pure, so the mapping is testable without a cycle.
fn halt_decision(outcome: &PollOutcome, recorded: Recorded) -> (PollOutcome, Option<String>) {
    match (outcome, recorded) {
        (PollOutcome::GuardTripped(reason), _) => (outcome.clone(), Some(reason.clone())),
        (_, Recorded::Escalate) => {
            let reason = "unclassifiable envelope x5".to_string();
            (PollOutcome::GuardTripped(reason.clone()), Some(reason))
        }
        _ => (outcome.clone(), None),
    }
}

/// Polls the accounts serially in D17 order. Returns when the cycle is done,
/// or early when a guard trip abandons the rest (same binary, same argv, same
/// fault). The `CycleToken` is dropped with this future, which clears busy.
async fn run_cycle(inputs: CycleInputs, _token: CycleToken) {
    let CycleInputs {
        core,
        events,
        machine,
        accounts,
        trigger,
        binary,
        cwd,
        timeout,
        pid_slot,
        cancel,
    } = inputs;

    info!(
        trigger = trigger.as_str(),
        accounts = accounts.len(),
        "cycle started"
    );

    let mut first_poll_logged_env = false;
    for account_id in accounts {
        if cancel.is_cancelled() {
            debug!("cycle cancelled before finishing");
            return;
        }

        let account = {
            let store_core = Arc::clone(&core);
            let id = account_id.clone();
            match blocking(move || store_core.store.account_by_id(&id)).await {
                Ok(Some(a)) => a,
                Ok(None) => continue,
                Err(e) => {
                    error!(account_id = %account_id, error = %e, "could not load account");
                    continue;
                }
            }
        };

        let taken_at = chrono::Utc::now().timestamp_millis();
        let result = run_usage(
            &binary,
            &account.config_dir,
            &cwd,
            timeout,
            chrono::Utc::now(),
            &pid_slot,
            &cancel,
            !first_poll_logged_env,
        )
        .await;
        first_poll_logged_env = true;

        if cancel.is_cancelled() {
            debug!(account_id = %account_id, "cycle cancelled mid-poll; result discarded");
            return;
        }

        // Backoff and the spec 6.3 step 5 streak are both updated by
        // `record`, which returns `Escalate` on the fifth consecutive
        // unclassifiable envelope. Publish the snapshot immediately: it is
        // the only scheduler state anything else can see.
        let recorded = lock_machine(&machine).record(&account_id, &result.outcome, taken_at);
        publish_status(&machine, &core.status, taken_at);

        let (outcome, halt_reason) = halt_decision(&result.outcome, recorded);

        if let Some(reason) = halt_reason {
            let sink = StoreHalt {
                core: Arc::clone(&core),
                account_id: account_id.clone(),
                outcome: outcome.clone(),
                raw: result.raw.clone(),
                taken_at,
                duration_ms: result.duration_ms,
            };
            let envelope = result
                .raw
                .clone()
                .unwrap_or_else(|| "<no stdout captured>".to_string());
            arm_and_perform_halt(&core, sink, taken_at, envelope, reason).await;
            events.usage_updated(&account_id);
            events.refresh_tray();
            events.cycle_finished();
            warn!("cycle abandoned after a guard trip");
            return;
        }

        {
            let store_core = Arc::clone(&core);
            let id = account_id.clone();
            let to_store = outcome.clone();
            let raw = result.raw.clone();
            let duration_ms = result.duration_ms;
            if let Err(e) = blocking(move || {
                store_core
                    .store
                    .insert_snapshot(&id, taken_at, &to_store, raw.as_deref(), duration_ms)
                    .map(|_| ())
            })
            .await
            {
                error!(account_id = %account_id, error = %e, "could not persist the snapshot");
            }
        }

        let kind = outcome.kind();

        if kind.is_failure() {
            warn!(
                account_id = %account_id,
                label = %account.label,
                outcome = kind.as_str(),
                duration_ms = result.duration_ms,
                trigger = trigger.as_str(),
                error = ?outcome.error_text(),
                "poll finished"
            );
        } else {
            info!(
                account_id = %account_id,
                label = %account.label,
                outcome = kind.as_str(),
                duration_ms = result.duration_ms,
                trigger = trigger.as_str(),
                "poll finished"
            );
        }

        events.usage_updated(&account_id);
        events.refresh_tray();
    }

    events.cycle_finished();
    info!(trigger = trigger.as_str(), "cycle finished");
}

pub struct Driver {
    core: Arc<Core>,
    events: Arc<dyn EventSink>,
    process: Arc<dyn ProcessProbe>,
    binary: Arc<dyn BinaryProbe>,
    shutdown: CancellationToken,
    pid_slot: Arc<AtomicU32>,
    /// Labels each cycle so a late "cycle finished" message can be matched
    /// against the cycle actually in flight.
    cycle_generation: AtomicU64,
    /// Owned here and nowhere else. Everything outside this file reads the
    /// `DriverStatus` snapshot instead (spec 5.1).
    machine: SharedMachine,
}

/// Tells the loop that a cycle task is over, so the driver republishes
/// `DriverStatus` immediately instead of at its next wake-up. Without it the
/// snapshot stays `busy` until the deadline or the next trigger, and
/// `preview_manual` refuses a Refresh click that should have run.
///
/// It signals from `Drop`, so a cycle that panics or is aborted reports too,
/// and it carries the cycle's generation so a message that arrives after the
/// watchdog has already reaped that cycle cannot reap its successor.
struct CycleDone {
    tx: UnboundedSender<u64>,
    generation: u64,
}

impl Drop for CycleDone {
    fn drop(&mut self) {
        let _ = self.tx.send(self.generation);
    }
}

struct LiveCycle {
    handle: tokio::task::JoinHandle<()>,
    cancel: CancellationToken,
    generation: u64,
}

impl Driver {
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

    fn current_pid(&self) -> Option<u32> {
        match self.pid_slot.load(Ordering::SeqCst) {
            0 => None,
            p => Some(p),
        }
    }

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

    /// Re-stat the binary and publish the result for `get_dashboard`.
    fn refresh_binary(&self, settings: &UserSettings) -> Option<PathBuf> {
        let found = self.binary.find(&settings.claude_binary);
        *lock_binary(&self.core.binary) = found
            .as_ref()
            .map(|(p, s)| (p.to_string_lossy().to_string(), *s));
        found.map(|(p, _)| p)
    }

    fn publish(&self) {
        publish_status(
            &self.machine,
            &self.core.status,
            chrono::Utc::now().timestamp_millis(),
        );
    }

    /// Spawns the cycle task described by a `Run` decision.
    fn start_cycle(
        &self,
        accounts: Vec<String>,
        trigger: Trigger,
        binary: PathBuf,
        settings: &UserSettings,
        done_tx: &UnboundedSender<u64>,
    ) -> LiveCycle {
        let generation = self.cycle_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let token = begin_cycle(&self.machine, chrono::Utc::now().timestamp_millis());
        // Busy has just become true; publish before the task starts so a
        // command arriving immediately sees it.
        self.publish();
        let cancel = self.shutdown.child_token();
        let inputs = CycleInputs {
            core: Arc::clone(&self.core),
            events: Arc::clone(&self.events),
            machine: Arc::clone(&self.machine),
            accounts,
            trigger,
            binary,
            cwd: crate::paths::poll_cwd(&self.core.app_data_dir),
            timeout: Duration::from_secs(u64::from(settings.timeout_secs)),
            pid_slot: Arc::clone(&self.pid_slot),
            cancel: cancel.clone(),
        };
        let signal = CycleDone {
            tx: done_tx.clone(),
            generation,
        };
        let handle = tokio::spawn(async move {
            // `signal` is declared first so it is dropped last: the cycle's
            // future — and with it the `CycleToken` that holds busy — is gone
            // before the loop is told, so the snapshot it republishes is
            // already correct.
            let _signal = signal;
            run_cycle(inputs, token).await;
        });
        LiveCycle {
            handle,
            cancel,
            generation,
        }
    }

    async fn settings(&self) -> UserSettings {
        let core = Arc::clone(&self.core);
        match blocking(move || core.store.stored_settings()).await {
            Ok(s) => s,
            Err(e) => {
                error!(error = %e, "could not read settings; using defaults");
                UserSettings {
                    interval_secs: crate::store::settings::DEFAULT_INTERVAL_SECS,
                    timeout_secs: crate::store::settings::DEFAULT_TIMEOUT_SECS,
                    claude_binary: String::new(),
                    close_to_tray: true,
                    launch_at_login: false,
                    log_level: "info".to_string(),
                }
            }
        }
    }

    /// An unreadable halt flag is treated as halted: the flag is the safety
    /// property, so the fail-closed answer is the only safe one.
    async fn halted(&self) -> bool {
        // A latched halt is authoritative on its own: it is raised before the
        // persist is attempted, so it covers the window in which the store
        // write has failed and `polling_halted` still reads `None` (F1).
        if self.core.halt_latched.load(Ordering::SeqCst) {
            return true;
        }
        let core = Arc::clone(&self.core);
        match blocking(move || core.store.polling_halted()).await {
            Ok(v) => v.is_some(),
            Err(e) => {
                error!(error = %e, "could not read the halt flag; assuming halted");
                true
            }
        }
    }

    async fn enabled(&self) -> Vec<String> {
        let core = Arc::clone(&self.core);
        match blocking(move || core.store.enabled_account_ids()).await {
            Ok(v) => v,
            Err(e) => {
                error!(error = %e, "could not list enabled accounts");
                Vec::new()
            }
        }
    }

    /// One `AccountChanged` notification.
    ///
    /// Draining is what consumes the ids, so it must not happen when the
    /// decision is already known to be `Skip(Busy)`: the ids would be thrown
    /// away and a just-enabled account would wait for the gate to open. The
    /// same is true of *every other* skip — halted, no binary, no enabled
    /// accounts — where the ids have already been drained into the trigger, so
    /// those are put straight back.
    ///
    /// They are put back silently. Re-notifying here instead would spin the
    /// select loop: the permit would be pending again on the next iteration,
    /// which would drain, skip and re-notify, once per iteration, with a
    /// store read each time. `deferred` is therefore what carries them, and
    /// `flush_deferred_changes` re-arms the notification at the next point
    /// where a decision can actually succeed.
    async fn handle_account_changed(
        &self,
        settings: &UserSettings,
        done_tx: &UnboundedSender<u64>,
        deferred: &mut bool,
    ) -> Option<LiveCycle> {
        if lock_machine(&self.machine).is_busy() {
            *deferred = true;
            debug!("account change deferred until the running cycle ends");
            return None;
        }
        // The permit and the id set are independent, so the set is drained
        // only after the permit has been consumed, and an empty drain is a
        // no-op rather than a decision.
        let ids = self.core.triggers.take_changed();
        if ids.is_empty() {
            return None;
        }
        let restore = ids.clone();
        let answer = self.probe_if_free().await;
        match self
            .decide_and_maybe_run(Trigger::AccountChanged(ids), answer, settings, done_tx)
            .await
        {
            Some(cycle) => Some(cycle),
            None => {
                self.core.triggers.defer_changed(restore);
                *deferred = true;
                debug!("account change skipped; its ids stay deferred");
                None
            }
        }
    }

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

    /// Runs one decision and, on `Run`, starts the cycle. Async because the
    /// enabled list and the halt flag both come from the store.
    async fn decide_and_maybe_run(
        &self,
        trigger: Trigger,
        claude_running: Option<bool>,
        settings: &UserSettings,
        done_tx: &UnboundedSender<u64>,
    ) -> Option<LiveCycle> {
        if self.shutdown.is_cancelled() {
            debug!(trigger = trigger.as_str(), "trigger ignored: shutting down");
            return None;
        }
        // A stat, not a database call, so it stays on this thread.
        let binary_path = self.refresh_binary(settings);
        let enabled = self.enabled().await;
        let halted = self.halted().await;
        let now = chrono::Utc::now().timestamp_millis();

        let decision = lock_machine(&self.machine).decide(
            trigger,
            claude_running,
            binary_path.is_some(),
            halted,
            &enabled,
            now,
        );
        // Spec 5.1: publish after every decide, so a skipped Manual trigger
        // and a gate transition are both visible to commands at once.
        self.publish();

        match decision {
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
            Decision::Run {
                accounts,
                reason,
                gate_transition,
            } => {
                if let Some(gate) = gate_transition {
                    info!(gate = gate.as_str(), trigger = reason.as_str(), "gate changed");
                    self.events.gate_changed(gate.as_str());
                }
                let binary = binary_path?;
                Some(self.start_cycle(accounts, reason, binary, settings, done_tx))
            }
        }
    }

    pub async fn run(self) {
        let mut settings_rx = self.core.settings_tx.subscribe();
        let mut settings = self.settings().await;

        if let Err(e) = crate::paths::ensure_dir(&crate::paths::poll_cwd(&self.core.app_data_dir)) {
            error!(error = %e, "could not create the poll working directory");
        }

        let mut last_prune = 0i64;
        let mut live: Option<LiveCycle> = None;
        // Cycle tasks report their own completion here, so the loop can
        // republish the snapshot the moment busy clears rather than at its
        // next wake-up. The driver keeps `done_tx` for its whole life, so
        // `recv` never returns `None` and can never spin the select.
        let (done_tx, mut done_rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
        // An account change that arrives mid-cycle is deferred, never
        // dropped: its ids stay accumulated in `Triggers` and are re-notified
        // when the cycle ends. Manual clicks are deliberately coalesced away
        // instead, but a newly enabled account must still get polled.
        let mut changed_deferred = false;
        let mut presence_deferred = false;

        // Startup: decide, run, then enter the loop.
        let startup_answer = self.probe_if_free().await;
        if let Some(cycle) = self
            .decide_and_maybe_run(Trigger::Startup, startup_answer, &settings, &done_tx)
            .await
        {
            live = Some(cycle);
        }

        let mut last_cycle_end = chrono::Utc::now().timestamp_millis();
        let mut watchdog = tokio::time::interval(WATCHDOG_TICK);
        watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // LOAD-BEARING INVARIANT (Task 12 review): `Machine::decide` and
        // `begin_cycle` are two separate lock acquisitions, and
        // `CycleToken::drop` clears busy without checking ownership. What
        // makes that safe is that THIS task is the only caller of `decide`
        // and `begin_cycle`, and it never calls them concurrently: commands
        // hold no `Machine` handle (they read the published `DriverStatus`
        // instead), and a cycle task only ever calls `record`. Do not call
        // either from a spawned task, and never drop a `CycleToken` while
        // holding the machine guard — that would deadlock in `Drop`.
        loop {
            // D10: prune at startup and every 24 h.
            let now = chrono::Utc::now().timestamp_millis();
            if now - last_prune >= PRUNE_INTERVAL_MS {
                last_prune = now;
                let core = Arc::clone(&self.core);
                if let Err(e) = blocking(move || core.store.prune(now)).await {
                    error!(error = %e, "prune failed");
                }
            }

            // Backstop reap. A cycle normally reports itself through
            // `done_rx`, which is prompter and also covers a panicking or
            // aborted task; this catches the one case that cannot report,
            // a task the runtime dropped before it ever ran.
            if live.as_ref().is_some_and(|c| c.handle.is_finished()) {
                live = None;
                last_cycle_end = chrono::Utc::now().timestamp_millis();
                lock_status(&self.core.status).stalled_at = None;
                // The token has dropped, so busy is false again.
                self.publish();
                flush_deferred_changes(&self.core, &mut changed_deferred);
                flush_deferred_presence(&self.core, &mut presence_deferred);
            }

            let deadline_ms = deadline_for(last_cycle_end, settings.interval_secs);
            let wait = Duration::from_millis(
                (deadline_ms - chrono::Utc::now().timestamp_millis()).max(0) as u64,
            );
            // Spec 6.5: the watchdog arm is disabled while no cycle is in
            // flight. `live.is_some()` is the same predicate as
            // `cycle_age(now).is_some()` without taking the machine lock
            // inside a select! precondition.
            let cycle_running = live.is_some();

            tokio::select! {
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
                changed = settings_rx.changed() => {
                    if changed.is_ok() {
                        settings = settings_rx.borrow_and_update().clone();
                        // Only a polling-relevant change reaches this arm at
                        // all (spec section 8), and D16 makes it a deliberate
                        // "try again" for every account.
                        lock_machine(&self.machine).reset_all_backoff();
                        self.publish();
                        info!(
                            interval_secs = settings.interval_secs,
                            timeout_secs = settings.timeout_secs,
                            "settings applied to the driver; backoff reset"
                        );
                        // The deadline moves, the clock is not restarted.
                    }
                }
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
                _ = self.core.triggers.notified_presence() => {
                    if let Some(cycle) = self
                        .handle_presence(&settings, &done_tx, &mut presence_deferred)
                        .await
                    {
                        live = Some(cycle);
                    }
                }
                _ = self.core.triggers.notified_changed() => {
                    if let Some(cycle) = self
                        .handle_account_changed(&settings, &done_tx, &mut changed_deferred)
                        .await
                    {
                        live = Some(cycle);
                    }
                }
                finished = done_rx.recv() => {
                    // Only the cycle currently in flight may be reaped: a
                    // message for an older generation is the tail of a cycle
                    // the watchdog has already dealt with.
                    if live.as_ref().is_some_and(|c| Some(c.generation) == finished) {
                        live = None;
                        last_cycle_end = chrono::Utc::now().timestamp_millis();
                        lock_status(&self.core.status).stalled_at = None;
                        self.publish();
                        flush_deferred_changes(&self.core, &mut changed_deferred);
                        flush_deferred_presence(&self.core, &mut presence_deferred);
                    }
                }
                _ = watchdog.tick(), if cycle_running => {
                    let age = lock_machine(&self.machine)
                        .cycle_age(chrono::Utc::now().timestamp_millis());
                    if let Some(age) = age {
                        let age_ms = age.as_millis() as u64;
                        let limit = watchdog_limit_ms(
                            self.enabled().await.len(),
                            settings.timeout_secs,
                        );
                        if age_ms > limit {
                            if let Some(cycle) = live.take() {
                                let at = chrono::Utc::now().timestamp_millis();
                                error!(
                                    cycle_age_ms = age_ms,
                                    limit_ms = limit,
                                    "watchdog: aborting a stalled cycle task"
                                );
                                cycle.cancel.cancel();
                                cycle.handle.abort();
                                // `abort()` only flags the task: the
                                // `CycleToken` inside it is dropped when the
                                // runtime actually unwinds it, which has not
                                // happened yet. Publishing now would write
                                // `busy: true` with no cycle left to clear
                                // it, and `preview_manual` would refuse every
                                // Refresh until the next timer tick. So wait
                                // for the join first; a cancelled join is the
                                // expected success.
                                if tokio::time::timeout(
                                    Duration::from_secs(2),
                                    cycle.handle,
                                )
                                .await
                                .is_err()
                                {
                                    warn!(
                                        "watchdog: the aborted cycle task did not \
                                         stop within 2 s"
                                    );
                                }
                                // An aborted poll never reaches `run_usage`'s
                                // own pid-clearing paths, so the slot is
                                // cleared here. A pid that has exited must
                                // never stay published as an exclusion: the
                                // OS can recycle it, and the gate would then
                                // ignore a real user `claude`.
                                self.pid_slot.store(0, Ordering::SeqCst);
                                lock_status(&self.core.status).stalled_at = Some(at);
                                last_cycle_end = at;
                                // The token has been observed dropping, so
                                // this snapshot is idle.
                                self.publish();
                                // Published before the event, so anything
                                // woken by `poller:stalled` reads the settled
                                // state rather than the stale one.
                                self.events.poller_stalled(at, age_ms);
                                flush_deferred_changes(
                                    &self.core,
                                    &mut changed_deferred,
                                );
                                flush_deferred_presence(
                                    &self.core,
                                    &mut presence_deferred,
                                );
                            }
                        }
                    }
                }
                _ = self.shutdown.cancelled() => {
                    info!("driver shutting down");
                    break;
                }
            }
        }

        // Shutdown: cancel the live cycle (which kills its child through the
        // child's own handle) and give it a bounded moment to finish.
        if let Some(cycle) = live {
            cycle.cancel.cancel();
            if tokio::time::timeout(Duration::from_secs(2), cycle.handle)
                .await
                .is_err()
            {
                warn!("cycle task did not finish within 2 s of cancellation");
            }
        }
        // One last snapshot so nothing is left reading a stale busy flag.
        self.publish();
        info!("driver stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `AppError` is only constructed by the recording halt sink below, so it
    // is imported here rather than at module scope, where it would be an
    // unused import in a non-test build and `-D warnings` would reject it.
    use crate::error::AppError;
    use crate::usage::UNEXPECTED_ENVELOPE_PREFIX;
    use std::cell::RefCell;

    // ---- shared fixtures for the loop-level tests -------------------------

    struct SilentEvents;
    impl EventSink for SilentEvents {
        fn usage_updated(&self, _account_id: &str) {}
        fn cycle_finished(&self) {}
        fn gate_changed(&self, _gate: &str) {}
        fn poller_stalled(&self, _at: i64, _cycle_age_ms: u64) {}
        fn refresh_tray(&self) {}
        fn system_sampled(&self) {}
    }

    struct IdleProcess;
    impl ProcessProbe for IdleProcess {
        fn claude_running(&self, _exclude_pid: Option<u32>) -> bool {
            false
        }
    }

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

    /// Always reports a binary, so a decision is never skipped for want of
    /// one. The path never has to exist: no test here lets a cycle spawn.
    struct FakeBinary(PathBuf);
    impl BinaryProbe for FakeBinary {
        fn find(&self, _override_path: &str) -> Option<(PathBuf, &'static str)> {
            Some((self.0.clone(), "override"))
        }
    }

    fn test_settings() -> UserSettings {
        UserSettings {
            interval_secs: 3600,
            timeout_secs: 5,
            claude_binary: String::new(),
            close_to_tray: true,
            launch_at_login: false,
            log_level: "info".to_string(),
        }
    }

    /// A `Core` on an in-memory store, with one enabled account.
    fn test_core() -> (tempfile::TempDir, Arc<Core>, String) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(crate::store::Store::open_in_memory().expect("open"));
        store.save_settings(&test_settings()).expect("seed settings");
        let dir = tmp.path().join(".claude3");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("settings.json"), "{}").expect("marker");
        let account = store
            .add_account(&dir, true, None, 1)
            .expect("add account");
        let (settings_tx, _rx) = tokio::sync::watch::channel(test_settings());
        let core = Arc::new(Core {
            store,
            triggers: Arc::new(crate::scheduler::triggers::Triggers::new()),
            status: Arc::new(Mutex::new(DriverStatus::default())),
            binary: Arc::new(Mutex::new(None)),
            halt_latched: std::sync::atomic::AtomicBool::new(false),
            close_to_tray: std::sync::atomic::AtomicBool::new(true),
            settings_tx,
            log: None,
            app_data_dir: tmp.path().to_path_buf(),
            log_dir: tmp.path().join("logs"),
        });
        let id = account.id.clone();
        (tmp, core, id)
    }

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

    /// A sink whose very first step fails, like a store that cannot be
    /// written. `Send` (unlike `RecordingHalt`) so it can cross the
    /// `blocking` hop.
    struct FailingHalt;
    impl HaltSink for FailingHalt {
        fn persist_halt(&self, _value: &str) -> AppResult<()> {
            Err(AppError::Db("disk on fire".into()))
        }
        fn log_envelope(&self, _raw: &str, _reason: &str) {}
        fn persist_outcome(&self) -> AppResult<()> {
            Ok(())
        }
        fn disable_account(&self) -> AppResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_failed_halt_write_still_latches_the_halt_and_stops_polling() {
        let (tmp, core, _id) = test_core();

        arm_and_perform_halt(
            &core,
            FailingHalt,
            1_700_000_000_000,
            "{}".to_string(),
            "no local_command".to_string(),
        )
        .await;

        assert_eq!(
            core.store.polling_halted().expect("read"),
            None,
            "the premise of this test is that the halt never reached disk"
        );
        assert!(
            core.halt_latched.load(Ordering::SeqCst),
            "the in-memory latch is what keeps the quota guard closed"
        );

        let driver = test_driver(Arc::clone(&core), tmp.path());
        assert!(driver.halted().await, "the driver must read the latch");
        assert_eq!(
            crate::commands::core_poll_now(&core).expect("poll"),
            "skipped:halted"
        );

        // ...and no further poll is started, by any trigger.
        let (done_tx, _done_rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
        for trigger in [Trigger::Manual, Trigger::Startup] {
            assert!(
                driver
                    .decide_and_maybe_run(trigger, None, &test_settings(), &done_tx)
                    .await
                    .is_none(),
                "a latched halt must skip every trigger"
            );
        }

        crate::commands::core_clear_halt(&core).expect("clear");
        assert!(!driver.halted().await, "clear_halt must reset the latch");
    }

    #[tokio::test]
    async fn an_account_changed_skipped_while_halted_keeps_its_ids() {
        let (tmp, core, id) = test_core();
        core.store.set_polling_halted("guard_tripped:1").expect("halt");
        core.triggers.account_changed(vec![id.clone()]);

        let driver = test_driver(Arc::clone(&core), tmp.path());
        let (done_tx, _done_rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
        let mut deferred = false;

        assert!(
            driver
                .handle_account_changed(&test_settings(), &done_tx, &mut deferred)
                .await
                .is_none(),
            "halted, so nothing runs"
        );
        assert!(deferred, "a skipped change must stay deferred");

        crate::commands::core_clear_halt(&core).expect("clear");

        let cycle = driver
            .handle_account_changed(&test_settings(), &done_tx, &mut deferred)
            .await;
        let cycle = cycle.expect("the id survived the halted skip, so this polls it now");
        cycle.cancel.cancel();
        cycle.handle.abort();
        let _ = cycle.handle.await;
    }

    #[tokio::test]
    async fn a_busy_account_changed_leaves_the_ids_undrained() {
        let (tmp, core, id) = test_core();
        core.triggers.account_changed(vec![id.clone()]);

        let driver = test_driver(Arc::clone(&core), tmp.path());
        let _token = begin_cycle(&driver.machine, 1);
        let (done_tx, _done_rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
        let mut deferred = false;

        assert!(driver
            .handle_account_changed(&test_settings(), &done_tx, &mut deferred)
            .await
            .is_none());
        assert!(deferred);
        assert_eq!(core.triggers.take_changed(), vec![id]);
    }

    #[test]
    fn the_deadline_is_always_the_last_cycle_end_plus_the_gap() {
        assert_eq!(deadline_for(1_000_000, 60), 1_000_000 + 60_000);
        assert_eq!(deadline_for(1_000_000, 10), 1_000_000 + 10_000);
        assert_eq!(deadline_for(1_000_000, 3600), 1_000_000 + 3_600_000);
    }

    #[test]
    fn changing_the_gap_moves_the_deadline_without_restarting_the_clock() {
        let last_cycle_end = 1_000_000i64;
        let before = deadline_for(last_cycle_end, 60);
        // 30 s later the user lowers the gap to 10 s.
        let after = deadline_for(last_cycle_end, 10);
        assert_eq!(before, 1_060_000);
        assert_eq!(
            after, 1_010_000,
            "the new deadline is measured from the same cycle end, not from now"
        );
    }

    #[test]
    fn the_watchdog_limit_scales_with_the_account_count() {
        // enabled x timeout_secs + 10 s
        assert_eq!(watchdog_limit_ms(1, 30), 40_000);
        assert_eq!(watchdog_limit_ms(3, 30), 100_000);
        assert_eq!(watchdog_limit_ms(0, 30), 10_000);
        assert_eq!(watchdog_limit_ms(3, 120), 370_000);
    }

    #[derive(Default)]
    struct RecordingHalt {
        steps: RefCell<Vec<&'static str>>,
        fail_on_persist_halt: bool,
    }

    impl HaltSink for RecordingHalt {
        fn persist_halt(&self, _value: &str) -> AppResult<()> {
            self.steps.borrow_mut().push("persist_halt");
            if self.fail_on_persist_halt {
                return Err(AppError::Db("disk on fire".into()));
            }
            Ok(())
        }
        fn log_envelope(&self, _raw: &str, _reason: &str) {
            self.steps.borrow_mut().push("log_envelope");
        }
        fn persist_outcome(&self) -> AppResult<()> {
            self.steps.borrow_mut().push("persist_outcome");
            Ok(())
        }
        fn disable_account(&self) -> AppResult<()> {
            self.steps.borrow_mut().push("disable_account");
            Ok(())
        }
    }

    #[test]
    fn a_halt_persists_the_flag_first_then_logs_then_persists_the_outcome() {
        let sink = RecordingHalt::default();
        perform_halt(&sink, 1_700_000_000_000, "{\"type\":\"result\"}", "no local_command")
            .expect("halt");
        assert_eq!(
            sink.steps.into_inner(),
            vec![
                "persist_halt",
                "log_envelope",
                "persist_outcome",
                "disable_account"
            ]
        );
    }

    #[test]
    fn a_failing_halt_flag_write_aborts_before_the_outcome_is_persisted() {
        let sink = RecordingHalt {
            fail_on_persist_halt: true,
            ..Default::default()
        };
        let err = perform_halt(&sink, 1, "{}", "no local_command").expect_err("must fail");
        assert_eq!(err.code(), "db");
        assert_eq!(
            sink.steps.into_inner(),
            vec!["persist_halt"],
            "the flag is the safety property; nothing else runs if it cannot be written"
        );
    }

    #[test]
    fn the_halt_value_is_the_documented_format() {
        assert_eq!(halt_value(1_700_000_000_000), "guard_tripped:1700000000000");
    }

    #[test]
    fn prune_runs_daily() {
        assert_eq!(PRUNE_INTERVAL_MS, 24 * 60 * 60 * 1000);
    }

    #[test]
    fn an_envelope_that_trips_the_guard_halts_with_its_own_reason() {
        let outcome = PollOutcome::GuardTripped("no local_command".to_string());
        let (stored, reason) = halt_decision(&outcome, Recorded::Continue);
        assert_eq!(reason.as_deref(), Some("no local_command"));
        assert!(matches!(stored, PollOutcome::GuardTripped(_)));
    }

    #[test]
    fn a_fifth_unclassifiable_envelope_is_stored_and_halted_as_a_guard_trip() {
        // `record` has already counted the strikes; the driver's job is to
        // turn `Escalate` into the same halt sequence a real trip takes.
        let outcome = PollOutcome::SpawnError(format!("{UNEXPECTED_ENVELOPE_PREFIX}no `type`"));
        let (stored, reason) = halt_decision(&outcome, Recorded::Escalate);
        assert_eq!(reason.as_deref(), Some("unclassifiable envelope x5"));
        match stored {
            PollOutcome::GuardTripped(r) => assert_eq!(r, "unclassifiable envelope x5"),
            other => panic!("expected the strike to be stored as a guard trip, got {other:?}"),
        }
    }

    #[test]
    fn an_ordinary_failure_is_stored_unchanged_and_never_halts() {
        let outcome = PollOutcome::Timeout(30);
        let (stored, reason) = halt_decision(&outcome, Recorded::Continue);
        assert_eq!(reason, None);
        assert_eq!(stored.kind().as_str(), PollOutcome::Timeout(30).kind().as_str());
    }

    #[test]
    fn publishing_copies_gate_busy_and_backoff_out_of_the_machine() {
        let machine: SharedMachine = Arc::new(Mutex::new(Machine::new()));
        let slot = Mutex::new(DriverStatus::default());
        let now = 1_700_000_000_000i64;

        lock_machine(&machine).record("a", &PollOutcome::Timeout(30), now);
        lock_machine(&machine).decide(
            Trigger::Timer,
            Some(true),
            true,
            false,
            &["b".to_string()],
            now,
        );

        publish_status(&machine, &slot, now);

        let published = lock_status(&slot).clone();
        assert_eq!(published.gate.as_str(), "active");
        assert!(!published.busy);
        assert_eq!(published.backoff_until.get("a"), Some(&(now + 60_000)));
    }

    #[test]
    fn publishing_preserves_the_watchdog_owned_stalled_at() {
        let machine: SharedMachine = Arc::new(Mutex::new(Machine::new()));
        let slot = Mutex::new(DriverStatus::default());
        lock_status(&slot).stalled_at = Some(4242);

        publish_status(&machine, &slot, 1);

        assert_eq!(
            lock_status(&slot).stalled_at,
            Some(4242),
            "Machine::status cannot know stalled_at, so it must be carried across"
        );
    }

    #[test]
    fn publishing_reports_a_running_cycle_as_busy() {
        let machine: SharedMachine = Arc::new(Mutex::new(Machine::new()));
        let slot = Mutex::new(DriverStatus::default());
        let token = begin_cycle(&machine, 1);
        publish_status(&machine, &slot, 1);
        assert!(lock_status(&slot).busy);

        drop(token);
        publish_status(&machine, &slot, 2);
        assert!(!lock_status(&slot).busy);
    }
}
