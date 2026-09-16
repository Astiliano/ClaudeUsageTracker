use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU32;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use cut_core::usage::runner::{check_envelope, run_usage, GuardVerdict, RunResult};
use cut_core::usage::{is_unexpected_envelope, PollOutcome};
use tokio_util::sync::CancellationToken;

fn fake() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake_claude"))
}

fn now() -> chrono::DateTime<chrono::Utc> {
    Utc.with_ymd_and_hms(2026, 9, 15, 20, 0, 0)
        .single()
        .expect("fixed instant")
}

async fn run_with(
    mode: &str,
    extra: &[(&str, &str)],
    timeout: Duration,
    cwd: &Path,
    config_dir: &Path,
) -> RunResult {
    // The fake binary is configured through the parent environment, exactly
    // as the real child inherits it.
    std::env::set_var("FAKE_CLAUDE_MODE", mode);
    for (k, v) in extra {
        std::env::set_var(k, v);
    }
    let pid = AtomicU32::new(0);
    let cancel = CancellationToken::new();
    run_usage(&fake(), config_dir, cwd, timeout, now(), &pid, &cancel, true).await
}

#[tokio::test]
async fn the_child_receives_exactly_the_spec_argv() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let r = run_with(
        "echo-argv",
        &[],
        Duration::from_secs(20),
        tmp.path(),
        tmp.path(),
    )
    .await;
    // echo-argv reports argv through the result text, which then fails the
    // usage parse; the raw envelope is what we assert on.
    let raw = r.raw.expect("raw stored");
    let v: serde_json::Value = serde_json::from_str(raw.trim()).expect("json");
    let args: Vec<String> = v["result"]
        .as_str()
        .unwrap_or_default()
        .split('\u{1f}')
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        args,
        vec![
            "-p",
            "/usage",
            "--output-format",
            "json",
            "--model",
            "haiku",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--permission-prompts",
            "none",
            "--safe-mode",
        ]
    );
}

#[tokio::test]
async fn the_child_environment_is_sanitised_and_the_config_dir_is_set() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg = tmp.path().join("cfg");
    std::fs::create_dir_all(&cfg).expect("mkdir");

    let r = run_with(
        "echo-env",
        &[
            ("ANTHROPIC_API_KEY", "leaked-key"),
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
            ("CLAUDE_CONFIG_DIR", "/wrong/dir"),
        ],
        Duration::from_secs(20),
        tmp.path(),
        &cfg,
    )
    .await;

    let raw = r.raw.expect("raw stored");
    let v: serde_json::Value = serde_json::from_str(raw.trim()).expect("json");
    let lines: Vec<String> = v["result"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .map(|s| s.to_string())
        .collect();

    assert!(
        lines.iter().all(|l| !l.starts_with("ANTHROPIC_")),
        "no ANTHROPIC_ variable may survive: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l == "CLAUDE_CODE_USE_BEDROCK=1"),
        "CLAUDE_ variables are stripped before the config dir is set"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.starts_with("CLAUDE_CONFIG_DIR="))
            .count(),
        1
    );
    assert!(lines
        .iter()
        .any(|l| l == &format!("CLAUDE_CONFIG_DIR={}", cfg.display())));
}

#[tokio::test]
async fn a_timeout_kills_the_child_and_records_the_limit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let started = std::time::Instant::now();
    let r = run_with(
        "slow",
        &[("FAKE_CLAUDE_SLEEP_SECS", "60")],
        Duration::from_secs(5),
        tmp.path(),
        tmp.path(),
    )
    .await;
    assert_eq!(r.outcome, PollOutcome::Timeout(5));
    assert_eq!(
        r.outcome.error_text(),
        Some("timed out after 5s".to_string())
    );
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "the runner must not wait out the child's full sleep"
    );
}

#[tokio::test]
async fn a_non_zero_exit_is_a_spawn_error_carrying_the_stderr_tail() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let r = run_with(
        "exit-nonzero",
        &[("FAKE_CLAUDE_EXIT", "7"), ("FAKE_CLAUDE_STDERR", "auth failed")],
        Duration::from_secs(20),
        tmp.path(),
        tmp.path(),
    )
    .await;
    match &r.outcome {
        PollOutcome::SpawnError(m) => {
            assert!(m.contains("exit"), "{m}");
            assert!(m.contains("auth failed"), "{m}");
        }
        other => panic!("expected SpawnError, got {other:?}"),
    }
}

#[tokio::test]
async fn exit_zero_with_non_json_stdout_is_a_spawn_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let r = run_with(
        "non-json",
        &[],
        Duration::from_secs(20),
        tmp.path(),
        tmp.path(),
    )
    .await;
    match &r.outcome {
        PollOutcome::SpawnError(m) => assert!(is_unexpected_envelope(m), "{m}"),
        other => panic!("expected SpawnError, got {other:?}"),
    }
}

#[tokio::test]
async fn a_missing_binary_is_a_spawn_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pid = AtomicU32::new(0);
    let cancel = CancellationToken::new();
    let r = run_usage(
        Path::new("/definitely/not/a/binary/claude-nope"),
        tmp.path(),
        tmp.path(),
        Duration::from_secs(20),
        now(),
        &pid,
        &cancel,
        false,
    )
    .await;
    match &r.outcome {
        PollOutcome::SpawnError(m) => assert!(m.contains("could not spawn"), "{m}"),
        other => panic!("expected SpawnError, got {other:?}"),
    }
}

#[tokio::test]
async fn a_good_usage_envelope_is_parsed_into_an_ok_outcome() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let report = "Current session: 15% used \u{b7} resets Sep 16, 3:30am (America/Los_Angeles)\\n\
                  Current week (all models): 4% used \u{b7} resets Sep 21, 8am (America/Los_Angeles)";
    let envelope = format!(
        r#"{{"type":"result","local_command":"usage","num_turns":0,"total_cost_usd":0,"result":"{report}"}}"#
    );
    let r = run_with(
        "emit",
        &[("FAKE_CLAUDE_STDOUT", &envelope)],
        Duration::from_secs(20),
        tmp.path(),
        tmp.path(),
    )
    .await;
    match &r.outcome {
        PollOutcome::Ok(p) => {
            assert_eq!(p.session.pct, 15);
            assert_eq!(p.week_all.pct, 4);
        }
        other => panic!("expected Ok, got {other:?}"),
    }
    assert_eq!(r.raw.as_deref(), Some(envelope.as_str()));
}

#[tokio::test]
async fn the_runner_never_logs_a_trip_itself() {
    // Spec 6.3 order: the halt flag reaches disk before anything is logged,
    // so the raw envelope must survive on the result for the driver's halt
    // sequence to log it there. This test pins the carrier, which is what
    // makes the "no logging in run_usage" rule safe.
    let tmp = tempfile::tempdir().expect("tempdir");
    let envelope = r#"{"type":"result","local_command":"cost","num_turns":0,"result":"x"}"#;
    let r = run_with(
        "emit",
        &[("FAKE_CLAUDE_STDOUT", envelope)],
        Duration::from_secs(20),
        tmp.path(),
        tmp.path(),
    )
    .await;
    assert!(matches!(r.outcome, PollOutcome::GuardTripped(_)));
    assert_eq!(
        r.raw.as_deref(),
        Some(envelope),
        "the driver's halt sequence is the only trip log site, so it needs these bytes"
    );
}

#[tokio::test]
async fn a_turn_envelope_trips_the_guard_and_keeps_the_raw_bytes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let envelope = r#"{"type":"result","num_turns":1,"total_cost_usd":0.75,"result":"hello"}"#;
    let r = run_with(
        "emit",
        &[("FAKE_CLAUDE_STDOUT", envelope)],
        Duration::from_secs(20),
        tmp.path(),
        tmp.path(),
    )
    .await;
    match &r.outcome {
        PollOutcome::GuardTripped(m) => assert!(m.contains("local_command"), "{m}"),
        other => panic!("expected GuardTripped, got {other:?}"),
    }
    assert_eq!(r.raw.as_deref(), Some(envelope));
}

#[tokio::test]
async fn a_not_logged_in_cost_summary_is_no_usage_data() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let envelope = r#"{"type":"result","local_command":"usage","num_turns":0,"total_cost_usd":0,"result":"Total cost: $0.0000"}"#;
    let r = run_with(
        "emit",
        &[("FAKE_CLAUDE_STDOUT", envelope)],
        Duration::from_secs(20),
        tmp.path(),
        tmp.path(),
    )
    .await;
    assert_eq!(r.outcome, PollOutcome::NoUsageData);
}

#[tokio::test]
async fn cancelling_kills_the_child_promptly() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::env::set_var("FAKE_CLAUDE_MODE", "slow");
    std::env::set_var("FAKE_CLAUDE_SLEEP_SECS", "60");
    let pid = AtomicU32::new(0);
    let cancel = CancellationToken::new();
    let child_cancel = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        child_cancel.cancel();
    });

    let started = std::time::Instant::now();
    let r = run_usage(
        &fake(),
        tmp.path(),
        tmp.path(),
        Duration::from_secs(120),
        now(),
        &pid,
        &cancel,
        false,
    )
    .await;
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "cancellation must not wait out the timeout"
    );
    match &r.outcome {
        PollOutcome::SpawnError(m) => assert!(m.contains("cancelled"), "{m}"),
        other => panic!("expected SpawnError(cancelled), got {other:?}"),
    }
}

#[tokio::test]
async fn the_child_pid_is_published_for_the_process_gate() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::env::set_var("FAKE_CLAUDE_MODE", "emit");
    std::env::set_var("FAKE_CLAUDE_STDOUT", r#"{"type":"result"}"#);
    let pid = AtomicU32::new(0);
    let cancel = CancellationToken::new();
    let _ = run_usage(
        &fake(),
        tmp.path(),
        tmp.path(),
        Duration::from_secs(20),
        now(),
        &pid,
        &cancel,
        false,
    )
    .await;
    assert_ne!(
        pid.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the runner must publish the child pid"
    );
}

#[test]
fn the_unexpected_envelope_prefix_is_shared_between_guard_and_predicate() {
    let v = check_envelope("not json");
    match v {
        GuardVerdict::Shape(m) => assert!(is_unexpected_envelope(&m)),
        other => panic!("expected Shape, got {other:?}"),
    }
    assert!(!is_unexpected_envelope("something else entirely"));
}
