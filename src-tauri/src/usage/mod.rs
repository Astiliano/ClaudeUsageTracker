pub mod parser;
pub mod runner;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Every shape-class guard message starts with this. It lives here rather
/// than in `runner.rs` because both the guard that produces it and the
/// scheduler machine that counts the streak (spec 6.3 step 5) need it, and
/// the machine must not depend on the runner.
pub const UNEXPECTED_ENVELOPE_PREFIX: &str = "unexpected envelope: ";

pub fn is_unexpected_envelope(message: &str) -> bool {
    message.starts_with(UNEXPECTED_ENVELOPE_PREFIX)
}

/// One quota window: a whole-percent usage figure and an optional reset instant
/// in epoch milliseconds UTC. `resets_at` is `None` when the CLI omitted the
/// reset clause (observed at 0 %, spec 2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub pct: u8,
    pub resets_at: Option<i64>,
}

/// A per-model weekly window, flattened for the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelWindow {
    pub label: String,
    pub pct: u8,
    pub resets_at: Option<i64>,
}

/// A successfully parsed usage report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub session: Window,
    pub week_all: Window,
    /// 0..n per-model lines, in the order the CLI printed them.
    pub week_models: Vec<(String, Window)>,
}

/// The wire/DB discriminant for a poll outcome. These six strings are used
/// identically in the DB column, the DTO, the logs and the UI (spec 5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeKind {
    Ok,
    NoUsageData,
    ParseError,
    SpawnError,
    Timeout,
    GuardTripped,
}

impl OutcomeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            OutcomeKind::Ok => "ok",
            OutcomeKind::NoUsageData => "no_usage_data",
            OutcomeKind::ParseError => "parse_error",
            OutcomeKind::SpawnError => "spawn_error",
            OutcomeKind::Timeout => "timeout",
            OutcomeKind::GuardTripped => "guard_tripped",
        }
    }

    pub fn from_wire(s: &str) -> Option<OutcomeKind> {
        match s {
            "ok" => Some(OutcomeKind::Ok),
            "no_usage_data" => Some(OutcomeKind::NoUsageData),
            "parse_error" => Some(OutcomeKind::ParseError),
            "spawn_error" => Some(OutcomeKind::SpawnError),
            "timeout" => Some(OutcomeKind::Timeout),
            "guard_tripped" => Some(OutcomeKind::GuardTripped),
            _ => None,
        }
    }

    /// Everything except `ok` is a failure for backoff (D16) and the pill.
    pub fn is_failure(&self) -> bool {
        !matches!(self, OutcomeKind::Ok)
    }
}

/// The result of one poll of one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    Ok(Parsed),
    /// Envelope fine, result text is not a usage report (e.g. not logged in).
    NoUsageData,
    /// Usage report detected, a required line failed.
    ParseError(String),
    /// Binary missing/not executable, non-zero exit, or stdout not JSON.
    SpawnError(String),
    /// Killed after `timeout_secs`; the payload is that limit in seconds.
    Timeout(u32),
    /// Spec 6.3 violated: the call may have reached the model.
    GuardTripped(String),
}

impl PollOutcome {
    pub fn kind(&self) -> OutcomeKind {
        match self {
            PollOutcome::Ok(_) => OutcomeKind::Ok,
            PollOutcome::NoUsageData => OutcomeKind::NoUsageData,
            PollOutcome::ParseError(_) => OutcomeKind::ParseError,
            PollOutcome::SpawnError(_) => OutcomeKind::SpawnError,
            PollOutcome::Timeout(_) => OutcomeKind::Timeout,
            PollOutcome::GuardTripped(_) => OutcomeKind::GuardTripped,
        }
    }

    /// The stored `error` column for this outcome, if any. A timeout formats
    /// its own message so the row explains itself (spec 5.1).
    pub fn error_text(&self) -> Option<String> {
        match self {
            PollOutcome::ParseError(m)
            | PollOutcome::SpawnError(m)
            | PollOutcome::GuardTripped(m) => Some(m.clone()),
            PollOutcome::Timeout(secs) => Some(format!("timed out after {secs}s")),
            PollOutcome::Ok(_) | PollOutcome::NoUsageData => None,
        }
    }
}

/// A persisted poll result. `raw` is kept for every outcome (D8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub id: i64,
    pub account_id: String,
    pub taken_at: i64,
    pub outcome: PollOutcome,
    pub raw: Option<String>,
    pub duration_ms: u32,
}

/// The flattened wire shape the frontend receives. Built by the store from a row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotDto {
    pub id: i64,
    pub account_id: String,
    pub taken_at: i64,
    pub outcome: &'static str,
    pub session: Option<Window>,
    pub week_all: Option<Window>,
    pub week_models: Vec<ModelWindow>,
    pub error: Option<String>,
    pub duration_ms: u32,
}

/// Why an account is disabled (spec 5.1). Wire form is snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisabledReason {
    User,
    GuardTripped,
}

impl DisabledReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            DisabledReason::User => "user",
            DisabledReason::GuardTripped => "guard_tripped",
        }
    }

    pub fn from_wire(s: &str) -> Option<DisabledReason> {
        match s {
            "user" => Some(DisabledReason::User),
            "guard_tripped" => Some(DisabledReason::GuardTripped),
            _ => None,
        }
    }
}

impl Serialize for DisabledReason {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// An account is a Claude Code config directory (D4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Account {
    pub id: String,
    pub label: String,
    pub config_dir: PathBuf,
    pub enabled: bool,
    pub disabled_reason: Option<DisabledReason>,
    pub created_at: i64,
    /// Manual order (D17, revised 2026-09-16): lower sorts first. Authoritative
    /// for `list_accounts`/`enabled_account_ids`; label is the only tiebreak.
    pub sort_order: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_wire_strings_round_trip() {
        let all = [
            (OutcomeKind::Ok, "ok"),
            (OutcomeKind::NoUsageData, "no_usage_data"),
            (OutcomeKind::ParseError, "parse_error"),
            (OutcomeKind::SpawnError, "spawn_error"),
            (OutcomeKind::Timeout, "timeout"),
            (OutcomeKind::GuardTripped, "guard_tripped"),
        ];
        for (kind, wire) in all {
            assert_eq!(kind.as_str(), wire);
            assert_eq!(OutcomeKind::from_wire(wire), Some(kind));
        }
        assert_eq!(OutcomeKind::from_wire("nope"), None);
    }

    #[test]
    fn only_ok_is_not_a_failure() {
        assert!(!OutcomeKind::Ok.is_failure());
        for k in [
            OutcomeKind::NoUsageData,
            OutcomeKind::ParseError,
            OutcomeKind::SpawnError,
            OutcomeKind::Timeout,
            OutcomeKind::GuardTripped,
        ] {
            assert!(k.is_failure(), "{} must count as a failure", k.as_str());
        }
    }

    #[test]
    fn poll_outcome_reports_its_kind_and_error_text() {
        let ok = PollOutcome::Ok(Parsed {
            session: Window { pct: 15, resets_at: Some(1_700_000_000_000) },
            week_all: Window { pct: 4, resets_at: None },
            week_models: vec![],
        });
        assert_eq!(ok.kind(), OutcomeKind::Ok);
        assert_eq!(ok.error_text(), None);

        let pe = PollOutcome::ParseError("missing session line".into());
        assert_eq!(pe.kind(), OutcomeKind::ParseError);
        assert_eq!(
            pe.error_text(),
            Some("missing session line".to_string())
        );

        assert_eq!(PollOutcome::NoUsageData.kind(), OutcomeKind::NoUsageData);
        assert_eq!(PollOutcome::NoUsageData.error_text(), None);
        assert_eq!(PollOutcome::Timeout(30).kind(), OutcomeKind::Timeout);
        assert_eq!(
            PollOutcome::Timeout(30).error_text(),
            Some("timed out after 30s".to_string())
        );
        assert_eq!(
            PollOutcome::SpawnError("boom".into()).error_text(),
            Some("boom".to_string())
        );
        assert_eq!(
            PollOutcome::GuardTripped("turn spent".into()).kind(),
            OutcomeKind::GuardTripped
        );
    }

    #[test]
    fn snapshot_dto_serialises_with_the_wire_field_names() {
        let dto = SnapshotDto {
            id: 7,
            account_id: "acct-1".into(),
            taken_at: 1_700_000_000_000,
            outcome: "ok",
            session: Some(Window { pct: 15, resets_at: Some(1_700_003_600_000) }),
            week_all: Some(Window { pct: 4, resets_at: None }),
            week_models: vec![ModelWindow {
                label: "Fable".into(),
                pct: 5,
                resets_at: Some(1_700_003_600_000),
            }],
            error: None,
            duration_ms: 3012,
        };
        let v = serde_json::to_value(&dto).expect("serialise");
        assert_eq!(v["id"], 7);
        assert_eq!(v["account_id"], "acct-1");
        assert_eq!(v["taken_at"], 1_700_000_000_000i64);
        assert_eq!(v["outcome"], "ok");
        assert_eq!(v["session"]["pct"], 15);
        assert_eq!(v["session"]["resets_at"], 1_700_003_600_000i64);
        assert!(v["week_all"]["resets_at"].is_null());
        assert_eq!(v["week_models"][0]["label"], "Fable");
        assert_eq!(v["week_models"][0]["pct"], 5);
        assert!(v["error"].is_null());
        assert_eq!(v["duration_ms"], 3012);
    }

    #[test]
    fn the_unexpected_envelope_prefix_is_shared_by_the_guard_and_the_machine() {
        assert_eq!(UNEXPECTED_ENVELOPE_PREFIX, "unexpected envelope: ");
        assert!(is_unexpected_envelope(
            "unexpected envelope: stdout is not JSON (expected value)"
        ));
        assert!(!is_unexpected_envelope("exit 7: auth failed"));
        assert!(!is_unexpected_envelope(""));
    }

    #[test]
    fn only_a_shape_class_spawn_error_counts_as_an_unexpected_envelope() {
        let shape = PollOutcome::SpawnError(format!(
            "{UNEXPECTED_ENVELOPE_PREFIX}no `type` field"
        ));
        assert!(shape
            .error_text()
            .map(|m| is_unexpected_envelope(&m))
            .unwrap_or(false));

        let other = PollOutcome::SpawnError("could not spawn /bin/nope".into());
        assert!(!other
            .error_text()
            .map(|m| is_unexpected_envelope(&m))
            .unwrap_or(false));
    }

    #[test]
    fn disabled_reason_uses_snake_case_wire_forms() {
        assert_eq!(DisabledReason::User.as_str(), "user");
        assert_eq!(DisabledReason::GuardTripped.as_str(), "guard_tripped");
        assert_eq!(DisabledReason::from_wire("user"), Some(DisabledReason::User));
        assert_eq!(
            DisabledReason::from_wire("guard_tripped"),
            Some(DisabledReason::GuardTripped)
        );
        assert_eq!(DisabledReason::from_wire("other"), None);
        assert_eq!(
            serde_json::to_value(DisabledReason::GuardTripped).expect("serialise"),
            serde_json::json!("guard_tripped")
        );
    }

    #[test]
    fn account_serialises_config_dir_as_a_string() {
        let a = Account {
            id: "acct-1".into(),
            label: "claude3".into(),
            config_dir: std::path::PathBuf::from("/home/josh/.claude3"),
            enabled: true,
            disabled_reason: None,
            created_at: 1_700_000_000_000,
            sort_order: 0,
        };
        let v = serde_json::to_value(&a).expect("serialise");
        assert_eq!(v["label"], "claude3");
        assert_eq!(v["enabled"], true);
        assert!(v["disabled_reason"].is_null());
        assert!(v["config_dir"].is_string());
        assert_eq!(v["sort_order"], 0);
    }
}
