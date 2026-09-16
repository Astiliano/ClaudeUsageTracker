use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use crate::usage::{is_unexpected_envelope, PollOutcome};

/// D16 backoff schedule: `min(900 s, 60 s * 2^(k-1))` on the k-th consecutive
/// non-`ok` outcome.
const BACKOFF_BASE_SECS: i64 = 60;
const BACKOFF_MAX_SECS: i64 = 900;

/// Spec 6.3 step 5: five consecutive unclassifiable envelopes on one account
/// escalate to a guard trip.
pub const MAX_ENVELOPE_STRIKES: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Idle,
    Active,
}

impl Gate {
    pub fn as_str(&self) -> &'static str {
        match self {
            Gate::Idle => "idle",
            Gate::Active => "active",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    Timer,
    Manual,
    Startup,
    AccountChanged(Vec<String>),
}

impl Trigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Trigger::Timer => "timer",
            Trigger::Manual => "manual",
            Trigger::Startup => "startup",
            Trigger::AccountChanged(_) => "account_changed",
        }
    }

    /// `Manual` and `AccountChanged` both ignore and reset backoff.
    fn bypasses_backoff(&self) -> bool {
        matches!(self, Trigger::Manual | Trigger::AccountChanged(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    Halted,
    Busy,
    NoBinary,
    NoEnabledAccounts,
    GateIdle,
    AllBackedOff,
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
        }
    }
}

/// What the driver must do. `decide` returns a description and performs no I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Run {
        accounts: Vec<String>,
        reason: Trigger,
        gate_transition: Option<Gate>,
    },
    Skip(SkipReason),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Backoff {
    pub consecutive_failures: u32,
    pub next_allowed: i64,
    /// Spec 6.3 step 5: consecutive `unexpected envelope` spawn errors.
    pub unexpected_envelope_streak: u8,
}

/// What `record` tells the cycle task to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorded {
    Continue,
    /// Spec 6.3 step 5 tripped: run the guard-trip sequence for this account.
    Escalate,
}

/// The snapshot the driver publishes into shared state after every `decide()`
/// and every `record()`. Spec 5.1: this is the only way code outside
/// `driver.rs` reads scheduler state, so `Machine`'s own fields are never
/// read across tasks and nothing can drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverStatus {
    pub gate: Gate,
    pub busy: bool,
    /// Owned by the watchdog, not by `Machine`; the driver carries the
    /// previous value across when it publishes a fresh snapshot.
    pub stalled_at: Option<i64>,
    pub backoff_until: HashMap<String, i64>,
}

impl Default for DriverStatus {
    fn default() -> Self {
        DriverStatus {
            gate: Gate::Idle,
            busy: false,
            stalled_at: None,
            backoff_until: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CycleInfo {
    started_at: i64,
}

pub struct Machine {
    gate: Gate,
    cycle: Option<CycleInfo>,
    backoff: HashMap<String, Backoff>,
}

impl Default for Machine {
    fn default() -> Self {
        Machine::new()
    }
}

impl Machine {
    pub fn new() -> Machine {
        Machine {
            gate: Gate::Idle,
            cycle: None,
            backoff: HashMap::new(),
        }
    }

    pub fn gate(&self) -> Gate {
        self.gate
    }

    pub fn is_busy(&self) -> bool {
        self.cycle.is_some()
    }

    /// The machine's only watchdog contribution.
    pub fn cycle_age(&self, now: i64) -> Option<Duration> {
        self.cycle
            .map(|c| Duration::from_millis((now - c.started_at).max(0) as u64))
    }

    pub fn backoff_until(&self, account: &str) -> Option<i64> {
        self.backoff
            .get(account)
            .filter(|b| b.consecutive_failures > 0)
            .map(|b| b.next_allowed)
    }

    /// D16: every outcome except `ok` extends the cooldown; `ok` clears it.
    /// Spec 6.3 step 5: a run of unclassifiable envelopes on one account is
    /// counted here too, and the escalation decision is made here so the
    /// cycle task only has to act on the returned `Recorded`.
    pub fn record(&mut self, account: &str, outcome: &PollOutcome, now: i64) -> Recorded {
        if !outcome.kind().is_failure() {
            // Removing the entry also clears the envelope streak.
            self.backoff.remove(account);
            return Recorded::Continue;
        }

        let is_envelope_error = matches!(
            outcome,
            PollOutcome::SpawnError(message) if is_unexpected_envelope(message)
        );

        let entry = self.backoff.entry(account.to_string()).or_default();
        entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
        let exponent = entry.consecutive_failures.saturating_sub(1).min(16);
        let delay_secs =
            (BACKOFF_BASE_SECS.saturating_mul(1i64 << exponent)).min(BACKOFF_MAX_SECS);
        entry.next_allowed = now + delay_secs * 1000;

        if is_envelope_error {
            entry.unexpected_envelope_streak =
                entry.unexpected_envelope_streak.saturating_add(1);
        } else {
            entry.unexpected_envelope_streak = 0;
        }

        if entry.unexpected_envelope_streak >= MAX_ENVELOPE_STRIKES {
            Recorded::Escalate
        } else {
            Recorded::Continue
        }
    }

    /// Snapshot for `AppState`. `stalled_at` is always `None` here: the
    /// watchdog owns it and the driver merges it in. `backoff_until` lists
    /// every account with a live failure streak rather than filtering on
    /// `now`, because the snapshot is written when the driver acts and read
    /// later by the UI, which already ignores an elapsed deadline.
    pub fn status(&self, _now: i64) -> DriverStatus {
        DriverStatus {
            gate: self.gate,
            busy: self.cycle.is_some(),
            stalled_at: None,
            backoff_until: self
                .backoff
                .iter()
                .filter(|(_, b)| b.consecutive_failures > 0)
                .map(|(id, b)| (id.clone(), b.next_allowed))
                .collect(),
        }
    }

    /// Clears the cooldown for one account, which is what a backoff-bypassing
    /// trigger (Manual, AccountChanged) means by "try again now".
    ///
    /// It deliberately does NOT clear `unexpected_envelope_streak`. Spec 6.3
    /// step 5 counts guard evidence, not retry policy: if a Refresh click
    /// reset the streak, five-strike protection would only ever fire on the
    /// unattended Timer path, and a user retrying a CLI that has started
    /// charging would keep it disarmed. The streak is cleared by exactly one
    /// thing, a non-strike outcome in `record`. An entry that has no streak
    /// left to carry is dropped, so the map does not accumulate.
    pub fn reset_backoff(&mut self, account: &str) {
        let spent = match self.backoff.get_mut(account) {
            Some(b) => {
                b.consecutive_failures = 0;
                b.next_allowed = 0;
                b.unexpected_envelope_streak == 0
            }
            None => return,
        };
        if spent {
            self.backoff.remove(account);
        }
    }

    /// Used by the settings watch (D16). Same rule as `reset_backoff`: every
    /// cooldown goes, every streak stays.
    pub fn reset_all_backoff(&mut self) {
        self.backoff.retain(|_, b| {
            b.consecutive_failures = 0;
            b.next_allowed = 0;
            b.unexpected_envelope_streak > 0
        });
    }

    fn end_cycle(&mut self) {
        self.cycle = None;
    }

    /// Pure in its arguments and the machine's fields; performs no I/O.
    /// Rules are evaluated in spec 6.5 order.
    pub fn decide(
        &mut self,
        trigger: Trigger,
        claude_running: Option<bool>,
        binary_present: bool,
        halted: bool,
        enabled: &[String],
        now: i64,
    ) -> Decision {
        // 0. A global guard halt beats every trigger, including Manual.
        if halted {
            return Decision::Skip(SkipReason::Halted);
        }
        // 1. One cycle at a time (D7).
        if self.cycle.is_some() {
            return Decision::Skip(SkipReason::Busy);
        }
        // 2/3. Nothing to spawn, or nothing to poll.
        if !binary_present {
            return Decision::Skip(SkipReason::NoBinary);
        }
        if enabled.is_empty() {
            return Decision::Skip(SkipReason::NoEnabledAccounts);
        }

        // 4. Candidate list.
        let mut next_gate = self.gate;
        let candidates: Vec<String> = match &trigger {
            Trigger::Timer => {
                let running = match claude_running {
                    Some(r) => r,
                    None => {
                        debug_assert!(
                            false,
                            "driver bug: a Timer decision needs a process-check answer"
                        );
                        return Decision::Skip(SkipReason::GateIdle);
                    }
                };
                match (self.gate, running) {
                    (Gate::Idle, false) => return Decision::Skip(SkipReason::GateIdle),
                    (Gate::Idle, true) => next_gate = Gate::Active,
                    (Gate::Active, false) => next_gate = Gate::Idle,
                    (Gate::Active, true) => {}
                }
                enabled.to_vec()
            }
            Trigger::Manual | Trigger::Startup => enabled.to_vec(),
            Trigger::AccountChanged(ids) => enabled
                .iter()
                .filter(|e| ids.iter().any(|i| i == *e))
                .cloned()
                .collect(),
        };

        // Resolution of a spec gap: an AccountChanged whose ids do not
        // intersect the enabled set has nothing to poll and nothing in
        // cooldown, so it is `no_enabled_accounts`. `all_backed_off` keeps its
        // spec meaning: enabled accounts exist and every one is in cooldown.
        if candidates.is_empty() {
            return Decision::Skip(SkipReason::NoEnabledAccounts);
        }

        // 5. Backoff filter (D16). Manual and AccountChanged reset instead.
        let runnable: Vec<String> = if trigger.bypasses_backoff() {
            for id in &candidates {
                self.reset_backoff(id);
            }
            candidates
        } else {
            let filtered: Vec<String> = candidates
                .into_iter()
                .filter(|id| match self.backoff.get(id) {
                    Some(b) if b.consecutive_failures > 0 => b.next_allowed <= now,
                    _ => true,
                })
                .collect();
            if filtered.is_empty() {
                // A skipped decision never moves the gate, so the promised
                // final poll cannot be lost to backoff.
                return Decision::Skip(SkipReason::AllBackedOff);
            }
            filtered
        };

        // 6. Only a Timer that actually runs moves the gate.
        let gate_transition = if matches!(trigger, Trigger::Timer) && next_gate != self.gate {
            self.gate = next_gate;
            Some(next_gate)
        } else {
            None
        };

        Decision::Run {
            accounts: runnable,
            reason: trigger,
            gate_transition,
        }
    }
}

/// Read-only answer to "would a Manual trigger run right now?", computed
/// from the published snapshot rather than from `Machine`. A Manual trigger
/// bypasses the gate and backoff, so only rules 0 to 3 can ever skip it,
/// which makes this preview exact. `poll_now` uses it to report
/// `skipped:<reason>` without consuming a trigger.
pub fn preview_manual(
    status: &DriverStatus,
    binary_present: bool,
    halted: bool,
    enabled: &[String],
) -> Option<SkipReason> {
    if halted {
        return Some(SkipReason::Halted);
    }
    if status.busy {
        return Some(SkipReason::Busy);
    }
    if !binary_present {
        return Some(SkipReason::NoBinary);
    }
    if enabled.is_empty() {
        return Some(SkipReason::NoEnabledAccounts);
    }
    None
}

pub type SharedMachine = Arc<Mutex<Machine>>;

/// A poisoned machine mutex means a cycle task panicked; the state itself is
/// still coherent, so recover rather than propagate the panic.
pub fn lock_machine(m: &Mutex<Machine>) -> MutexGuard<'_, Machine> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// RAII busy marker. Dropping it — including on panic or task abort — clears
/// busy, so the driver never has to clear it by hand and two cycles can never
/// overlap.
pub struct CycleToken {
    machine: Weak<Mutex<Machine>>,
}

impl Drop for CycleToken {
    fn drop(&mut self) {
        if let Some(m) = self.machine.upgrade() {
            lock_machine(&m).end_cycle();
        }
    }
}

/// D7: `decide` and `begin_cycle` are the single entry point, always called
/// together by the driver.
pub fn begin_cycle(shared: &SharedMachine, now: i64) -> CycleToken {
    lock_machine(shared).cycle = Some(CycleInfo { started_at: now });
    CycleToken {
        machine: Arc::downgrade(shared),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{Parsed, Window, UNEXPECTED_ENVELOPE_PREFIX};
    use std::sync::{Arc, Mutex};

    const NOW: i64 = 1_700_000_000_000;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn ok_outcome() -> PollOutcome {
        PollOutcome::Ok(Parsed {
            session: Window { pct: 1, resets_at: None },
            week_all: Window { pct: 1, resets_at: None },
            week_models: vec![],
        })
    }

    /// A failure that is not a shape-class envelope error.
    fn plain_failure() -> PollOutcome {
        PollOutcome::Timeout(30)
    }

    /// A spawn error the guard could not classify (spec 6.3 step 5).
    fn shape_failure() -> PollOutcome {
        PollOutcome::SpawnError(format!("{UNEXPECTED_ENVELOPE_PREFIX}no `type` field"))
    }

    fn accounts() -> Vec<String> {
        ids(&["a", "b"])
    }

    fn run_accounts(d: &Decision) -> Vec<String> {
        match d {
            Decision::Run { accounts, .. } => accounts.clone(),
            Decision::Skip(r) => panic!("expected Run, got Skip({})", r.as_str()),
        }
    }

    #[test]
    fn wire_forms_are_snake_case() {
        assert_eq!(Gate::Idle.as_str(), "idle");
        assert_eq!(Gate::Active.as_str(), "active");
        assert_eq!(Trigger::Timer.as_str(), "timer");
        assert_eq!(Trigger::Manual.as_str(), "manual");
        assert_eq!(Trigger::Startup.as_str(), "startup");
        assert_eq!(Trigger::AccountChanged(vec![]).as_str(), "account_changed");
        assert_eq!(SkipReason::Halted.as_str(), "halted");
        assert_eq!(SkipReason::Busy.as_str(), "busy");
        assert_eq!(SkipReason::NoBinary.as_str(), "no_binary");
        assert_eq!(
            SkipReason::NoEnabledAccounts.as_str(),
            "no_enabled_accounts"
        );
        assert_eq!(SkipReason::GateIdle.as_str(), "gate_idle");
        assert_eq!(SkipReason::AllBackedOff.as_str(), "all_backed_off");
    }

    #[test]
    fn halted_beats_every_trigger_including_manual() {
        let mut m = Machine::new();
        for t in [
            Trigger::Timer,
            Trigger::Manual,
            Trigger::Startup,
            Trigger::AccountChanged(ids(&["a"])),
        ] {
            let d = m.decide(t, Some(true), true, true, &accounts(), NOW);
            assert_eq!(d, Decision::Skip(SkipReason::Halted));
        }
    }

    #[test]
    fn busy_is_checked_before_the_process_state() {
        let shared: SharedMachine = Arc::new(Mutex::new(Machine::new()));
        let _token = begin_cycle(&shared, NOW);
        let mut m = lock_machine(&shared);
        // claude_running is None, which would be a bug for a Timer — busy
        // must win before that is ever consulted.
        let d = m.decide(Trigger::Timer, None, true, false, &accounts(), NOW);
        assert_eq!(d, Decision::Skip(SkipReason::Busy));
    }

    #[test]
    fn no_binary_skips_every_trigger() {
        let mut m = Machine::new();
        for t in [
            Trigger::Timer,
            Trigger::Manual,
            Trigger::Startup,
            Trigger::AccountChanged(ids(&["a"])),
        ] {
            let d = m.decide(t, Some(true), false, false, &accounts(), NOW);
            assert_eq!(d, Decision::Skip(SkipReason::NoBinary));
        }
    }

    #[test]
    fn no_enabled_accounts_skips_every_trigger() {
        let mut m = Machine::new();
        for t in [Trigger::Timer, Trigger::Manual, Trigger::Startup] {
            let d = m.decide(t, Some(true), true, false, &[], NOW);
            assert_eq!(d, Decision::Skip(SkipReason::NoEnabledAccounts));
        }
    }

    #[test]
    fn timer_decision_table_for_all_four_gate_and_running_combinations() {
        // Idle + not running: stay idle, skip.
        let mut m = Machine::new();
        assert_eq!(
            m.decide(Trigger::Timer, Some(false), true, false, &accounts(), NOW),
            Decision::Skip(SkipReason::GateIdle)
        );
        assert_eq!(m.gate(), Gate::Idle);

        // Idle + running: run and switch to active.
        let mut m = Machine::new();
        let d = m.decide(Trigger::Timer, Some(true), true, false, &accounts(), NOW);
        assert_eq!(run_accounts(&d), accounts());
        assert!(matches!(
            d,
            Decision::Run { gate_transition: Some(Gate::Active), .. }
        ));
        assert_eq!(m.gate(), Gate::Active);

        // Active + running: run, no transition.
        let d = m.decide(Trigger::Timer, Some(true), true, false, &accounts(), NOW);
        assert!(matches!(d, Decision::Run { gate_transition: None, .. }));
        assert_eq!(m.gate(), Gate::Active);

        // Active + not running: the final poll, then back to idle.
        let d = m.decide(Trigger::Timer, Some(false), true, false, &accounts(), NOW);
        assert!(matches!(
            d,
            Decision::Run { gate_transition: Some(Gate::Idle), .. }
        ));
        assert_eq!(m.gate(), Gate::Idle);
    }

    #[test]
    fn a_timer_without_a_process_answer_skips_as_gate_idle() {
        // Release mode only: in debug this path trips a debug_assert.
        if cfg!(debug_assertions) {
            return;
        }
        let mut m = Machine::new();
        assert_eq!(
            m.decide(Trigger::Timer, None, true, false, &accounts(), NOW),
            Decision::Skip(SkipReason::GateIdle)
        );
    }

    #[test]
    fn the_final_poll_happens_exactly_once() {
        let mut m = Machine::new();
        let mut runs = 0;
        // running, then stopped, then stopped again.
        for running in [true, false, false] {
            if let Decision::Run { .. } =
                m.decide(Trigger::Timer, Some(running), true, false, &accounts(), NOW)
            {
                runs += 1;
            }
        }
        assert_eq!(runs, 2, "one active poll plus exactly one final poll");
        assert_eq!(m.gate(), Gate::Idle);
    }

    #[test]
    fn manual_and_startup_ignore_the_gate() {
        let mut m = Machine::new();
        let d = m.decide(Trigger::Manual, Some(false), true, false, &accounts(), NOW);
        assert_eq!(run_accounts(&d), accounts());
        assert_eq!(m.gate(), Gate::Idle, "manual never moves the gate");

        let d = m.decide(Trigger::Startup, Some(false), true, false, &accounts(), NOW);
        assert_eq!(run_accounts(&d), accounts());
        assert_eq!(m.gate(), Gate::Idle);
    }

    #[test]
    fn account_changed_runs_the_intersection_with_enabled() {
        let mut m = Machine::new();
        let d = m.decide(
            Trigger::AccountChanged(ids(&["b", "zzz"])),
            Some(false),
            true,
            false,
            &accounts(),
            NOW,
        );
        assert_eq!(run_accounts(&d), ids(&["b"]));
    }

    #[test]
    fn account_changed_with_an_empty_intersection_skips_as_no_enabled_accounts() {
        let mut m = Machine::new();
        let d = m.decide(
            Trigger::AccountChanged(ids(&["zzz"])),
            Some(true),
            true,
            false,
            &accounts(),
            NOW,
        );
        assert_eq!(d, Decision::Skip(SkipReason::NoEnabledAccounts));
    }

    #[test]
    fn backoff_schedule_doubles_and_caps_at_fifteen_minutes() {
        let mut m = Machine::new();
        let expected_secs = [60i64, 120, 240, 480, 900, 900, 900];
        for (i, want) in expected_secs.iter().enumerate() {
            assert_eq!(m.record("a", &plain_failure(), NOW), Recorded::Continue);
            assert_eq!(
                m.backoff_until("a"),
                Some(NOW + want * 1000),
                "failure number {}",
                i + 1
            );
        }
    }

    #[test]
    fn every_non_ok_outcome_counts_as_a_failure_for_backoff() {
        for outcome in [
            PollOutcome::NoUsageData,
            PollOutcome::ParseError("x".into()),
            PollOutcome::SpawnError("could not spawn".into()),
            PollOutcome::Timeout(30),
            PollOutcome::GuardTripped("x".into()),
        ] {
            let mut m = Machine::new();
            assert!(outcome.kind().is_failure());
            m.record("a", &outcome, NOW);
            assert_eq!(m.backoff_until("a"), Some(NOW + 60_000));
        }
    }

    #[test]
    fn an_ok_outcome_clears_the_backoff() {
        let mut m = Machine::new();
        m.record("a", &plain_failure(), NOW);
        m.record("a", &plain_failure(), NOW);
        assert_eq!(m.record("a", &ok_outcome(), NOW), Recorded::Continue);
        assert_eq!(m.backoff_until("a"), None);
        m.record("a", &plain_failure(), NOW);
        assert_eq!(
            m.backoff_until("a"),
            Some(NOW + 60_000),
            "the failure count restarts at one"
        );
    }

    #[test]
    fn five_consecutive_unclassifiable_envelopes_escalate() {
        let mut m = Machine::new();
        for i in 1..MAX_ENVELOPE_STRIKES {
            assert_eq!(
                m.record("a", &shape_failure(), NOW),
                Recorded::Continue,
                "strike {i} must not escalate yet"
            );
        }
        assert_eq!(
            m.record("a", &shape_failure(), NOW),
            Recorded::Escalate,
            "the fifth consecutive strike escalates"
        );
    }

    #[test]
    fn the_envelope_streak_is_tracked_per_account() {
        let mut m = Machine::new();
        for _ in 0..4 {
            assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Continue);
            assert_eq!(m.record("b", &shape_failure(), NOW), Recorded::Continue);
        }
        assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Escalate);
        assert_eq!(m.record("b", &shape_failure(), NOW), Recorded::Escalate);
    }

    #[test]
    fn any_other_outcome_resets_the_envelope_streak() {
        let mut m = Machine::new();
        for _ in 0..4 {
            assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Continue);
        }
        // A different failure still backs off, but it breaks the run.
        assert_eq!(m.record("a", &plain_failure(), NOW), Recorded::Continue);
        for _ in 0..4 {
            assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Continue);
        }
        assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Escalate);
    }

    #[test]
    fn a_success_also_resets_the_envelope_streak() {
        let mut m = Machine::new();
        for _ in 0..4 {
            assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Continue);
        }
        assert_eq!(m.record("a", &ok_outcome(), NOW), Recorded::Continue);
        for _ in 0..4 {
            assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Continue);
        }
        assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Escalate);
    }

    #[test]
    fn a_spawn_error_that_is_not_an_envelope_problem_never_escalates() {
        let mut m = Machine::new();
        for _ in 0..20 {
            assert_eq!(
                m.record("a", &PollOutcome::SpawnError("exit 7: boom".into()), NOW),
                Recorded::Continue
            );
        }
    }

    #[test]
    fn status_snapshots_gate_busy_and_every_cooling_account() {
        let shared: SharedMachine = Arc::new(Mutex::new(Machine::new()));
        {
            let mut m = lock_machine(&shared);
            m.record("a", &plain_failure(), NOW);
            let status = m.status(NOW);
            assert_eq!(status.gate, Gate::Idle);
            assert!(!status.busy);
            assert_eq!(status.stalled_at, None);
            assert_eq!(status.backoff_until.get("a"), Some(&(NOW + 60_000)));
            assert_eq!(status.backoff_until.get("b"), None);
        }

        let _token = begin_cycle(&shared, NOW);
        let status = lock_machine(&shared).status(NOW);
        assert!(status.busy, "status must report a running cycle");
    }

    #[test]
    fn status_reports_the_active_gate_after_a_timer_run() {
        let mut m = Machine::new();
        m.decide(Trigger::Timer, Some(true), true, false, &accounts(), NOW);
        assert_eq!(m.status(NOW).gate, Gate::Active);
    }

    #[test]
    fn status_drops_an_account_once_its_backoff_is_cleared() {
        let mut m = Machine::new();
        m.record("a", &plain_failure(), NOW);
        m.record("a", &ok_outcome(), NOW);
        assert!(m.status(NOW).backoff_until.is_empty());
    }

    #[test]
    fn preview_manual_reads_only_the_published_status() {
        let mut m = Machine::new();
        m.record("a", &plain_failure(), NOW);
        let idle = m.status(NOW);

        assert_eq!(
            preview_manual(&idle, true, true, &accounts()),
            Some(SkipReason::Halted)
        );
        assert_eq!(
            preview_manual(&idle, false, false, &accounts()),
            Some(SkipReason::NoBinary)
        );
        assert_eq!(
            preview_manual(&idle, true, false, &[]),
            Some(SkipReason::NoEnabledAccounts)
        );
        assert_eq!(
            preview_manual(&idle, true, false, &accounts()),
            None,
            "a manual trigger bypasses backoff, so a cooling account cannot skip it"
        );

        let busy = DriverStatus {
            busy: true,
            ..idle.clone()
        };
        assert_eq!(
            preview_manual(&busy, true, false, &accounts()),
            Some(SkipReason::Busy)
        );
    }

    #[test]
    fn preview_manual_orders_halted_ahead_of_busy_and_no_binary() {
        let status = DriverStatus {
            gate: Gate::Active,
            busy: true,
            stalled_at: None,
            backoff_until: HashMap::new(),
        };
        assert_eq!(
            preview_manual(&status, false, true, &[]),
            Some(SkipReason::Halted)
        );
        assert_eq!(
            preview_manual(&status, false, false, &[]),
            Some(SkipReason::Busy)
        );
    }

    #[test]
    fn a_timer_drops_backed_off_accounts() {
        let mut m = Machine::new();
        m.record("a", &plain_failure(), NOW);
        let d = m.decide(
            Trigger::Timer,
            Some(true),
            true,
            false,
            &accounts(),
            NOW + 1000,
        );
        assert_eq!(run_accounts(&d), ids(&["b"]));
    }

    #[test]
    fn all_backed_off_means_every_enabled_account_is_in_cooldown() {
        let mut m = Machine::new();
        m.record("a", &plain_failure(), NOW);
        m.record("b", &plain_failure(), NOW);
        let d = m.decide(
            Trigger::Timer,
            Some(true),
            true,
            false,
            &accounts(),
            NOW + 1000,
        );
        assert_eq!(d, Decision::Skip(SkipReason::AllBackedOff));
    }

    #[test]
    fn manual_ignores_and_resets_backoff() {
        let mut m = Machine::new();
        m.record("a", &plain_failure(), NOW);
        m.record("b", &plain_failure(), NOW);
        let d = m.decide(
            Trigger::Manual,
            Some(false),
            true,
            false,
            &accounts(),
            NOW + 1000,
        );
        assert_eq!(run_accounts(&d), accounts());
        assert_eq!(m.backoff_until("a"), None);
        assert_eq!(m.backoff_until("b"), None);
    }

    #[test]
    fn account_changed_ignores_and_resets_backoff_for_its_accounts_only() {
        let mut m = Machine::new();
        m.record("a", &plain_failure(), NOW);
        m.record("b", &plain_failure(), NOW);
        let d = m.decide(
            Trigger::AccountChanged(ids(&["a"])),
            Some(false),
            true,
            false,
            &accounts(),
            NOW + 1000,
        );
        assert_eq!(run_accounts(&d), ids(&["a"]));
        assert_eq!(m.backoff_until("a"), None);
        assert_eq!(
            m.backoff_until("b"),
            Some(NOW + 60_000),
            "an untouched account keeps its cooldown"
        );
    }

    #[test]
    fn reset_backoff_clears_one_account_and_leaves_the_rest() {
        let mut m = Machine::new();
        m.record("a", &plain_failure(), NOW);
        m.record("b", &plain_failure(), NOW);

        m.reset_backoff("a");
        assert_eq!(m.backoff_until("a"), None);
        assert_eq!(m.backoff_until("b"), Some(NOW + 60_000));

        // Resetting an account that was never recorded is a no-op.
        m.reset_backoff("never-seen");
        assert_eq!(m.backoff_until("b"), Some(NOW + 60_000));
    }

    /// Spec 6.3 step 5: the streak is guard evidence, not a cooldown. A user
    /// clicking Refresh against a CLI that has started charging must not be
    /// able to erase it, or the five-strike protection would only ever work
    /// on the unattended Timer path.
    #[test]
    fn reset_backoff_preserves_the_envelope_streak() {
        let mut m = Machine::new();
        for _ in 0..4 {
            assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Continue);
        }
        m.reset_backoff("a");
        assert_eq!(
            m.backoff_until("a"),
            None,
            "the cooldown itself is still cleared"
        );
        assert_eq!(
            m.record("a", &shape_failure(), NOW),
            Recorded::Escalate,
            "the fifth strike still escalates across a manual retry"
        );
    }

    #[test]
    fn reset_all_backoff_preserves_the_envelope_streak() {
        let mut m = Machine::new();
        for _ in 0..4 {
            assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Continue);
        }
        // The settings watch resets every cooldown (D16), which must not
        // amount to a way of disarming the guard.
        m.reset_all_backoff();
        assert_eq!(m.backoff_until("a"), None);
        assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Escalate);
    }

    #[test]
    fn only_a_non_strike_outcome_clears_the_envelope_streak() {
        let mut m = Machine::new();
        for _ in 0..4 {
            assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Continue);
        }
        m.reset_backoff("a");
        assert_eq!(m.record("a", &ok_outcome(), NOW), Recorded::Continue);
        for _ in 0..4 {
            assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Continue);
        }
        assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Escalate);
    }

    #[test]
    fn reset_all_backoff_clears_everything() {
        let mut m = Machine::new();
        m.record("a", &plain_failure(), NOW);
        m.record("b", &plain_failure(), NOW);
        m.reset_all_backoff();
        assert_eq!(m.backoff_until("a"), None);
        assert_eq!(m.backoff_until("b"), None);
    }

    #[test]
    fn the_gate_does_not_move_on_a_skipped_decision() {
        let mut m = Machine::new();
        // Get to Active.
        m.decide(Trigger::Timer, Some(true), true, false, &accounts(), NOW);
        assert_eq!(m.gate(), Gate::Active);
        // Everything is in cooldown when the final poll would be due.
        m.record("a", &plain_failure(), NOW);
        m.record("b", &plain_failure(), NOW);
        let d = m.decide(
            Trigger::Timer,
            Some(false),
            true,
            false,
            &accounts(),
            NOW + 1000,
        );
        assert_eq!(d, Decision::Skip(SkipReason::AllBackedOff));
        assert_eq!(
            m.gate(),
            Gate::Active,
            "the promised final poll must not be lost to backoff"
        );
        // Once the cooldown expires the final poll still happens.
        let d = m.decide(
            Trigger::Timer,
            Some(false),
            true,
            false,
            &accounts(),
            NOW + 61_000,
        );
        assert!(matches!(
            d,
            Decision::Run { gate_transition: Some(Gate::Idle), .. }
        ));
    }

    #[test]
    fn a_cycle_token_marks_the_machine_busy_and_clears_it_on_drop() {
        let shared: SharedMachine = Arc::new(Mutex::new(Machine::new()));
        assert!(!lock_machine(&shared).is_busy());
        {
            let _token = begin_cycle(&shared, NOW);
            assert!(lock_machine(&shared).is_busy());
        }
        assert!(!lock_machine(&shared).is_busy());
    }

    #[test]
    fn a_panic_while_holding_the_token_still_clears_busy() {
        let shared: SharedMachine = Arc::new(Mutex::new(Machine::new()));
        let inner = Arc::clone(&shared);
        let result = std::panic::catch_unwind(move || {
            let _token = begin_cycle(&inner, NOW);
            panic!("cycle task exploded");
        });
        assert!(result.is_err());
        assert!(
            !lock_machine(&shared).is_busy(),
            "Drop must clear busy even on panic"
        );
    }

    #[test]
    fn cycle_age_is_none_when_idle_and_grows_while_a_cycle_runs() {
        let shared: SharedMachine = Arc::new(Mutex::new(Machine::new()));
        assert_eq!(lock_machine(&shared).cycle_age(NOW), None);
        let _token = begin_cycle(&shared, NOW);
        assert_eq!(
            lock_machine(&shared).cycle_age(NOW + 5000),
            Some(std::time::Duration::from_millis(5000))
        );
        assert_eq!(
            lock_machine(&shared).cycle_age(NOW - 5000),
            Some(std::time::Duration::from_millis(0)),
            "a backwards clock must not underflow"
        );
    }
}
