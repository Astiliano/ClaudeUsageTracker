use serde_json::Value;

use super::UNEXPECTED_ENVELOPE_PREFIX;

/// Spec 2.2 / 6.3. The flag set lives in exactly one place. `--bare` must
/// never appear here: it does not read OAuth credentials.
pub const USAGE_ARGV: [&str; 11] = [
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
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardVerdict {
    /// A confirmed local `/usage` envelope; the payload is the `result` text.
    Usage(String),
    /// The envelope could not be classified. Retryable; backs off.
    Shape(String),
    /// Turn evidence. Halts the whole poller.
    Tripped(String),
}

/// Every shape-class message carries the shared prefix, which is how
/// `Machine::record` recognises a strike for the spec 6.3 step 5 escalation.
fn shape(reason: &str) -> GuardVerdict {
    GuardVerdict::Shape(format!("{UNEXPECTED_ENVELOPE_PREFIX}{reason}"))
}

/// The envelope guard, evaluated strictly in spec 6.3 order.
pub fn check_envelope(stdout: &str) -> GuardVerdict {
    // 1. Is this even a result envelope?
    let v: Value = match serde_json::from_str(stdout.trim()) {
        Ok(v) => v,
        Err(e) => return shape(&format!("stdout is not JSON ({e})")),
    };
    let type_field = match v.get("type").and_then(Value::as_str) {
        Some(t) => t,
        None => return shape("no `type` field"),
    };
    if type_field != "result" {
        return shape(&format!("type is \"{type_field}\", expected \"result\""));
    }

    // 2. Primary turn evidence. Checked before anything else about the
    //    envelope's shape, so a malformed turn envelope can never be
    //    misclassified as a retryable shape problem.
    match v.get("local_command").and_then(Value::as_str) {
        Some("usage") => {}
        Some(other) => {
            return GuardVerdict::Tripped(format!(
                "local_command is \"{other}\", expected \"usage\" — a turn may have been spent"
            ))
        }
        None => {
            return GuardVerdict::Tripped(
                "result envelope has no `local_command` field — a model turn was spent".to_string(),
            )
        }
    }

    // 3. Advisory evidence: adds trips, never excuses one. Under subscription
    //    auth the cost can read 0 for a billed turn, which is why rule 2 is
    //    the primary signal.
    if let Some(n) = v.get("num_turns").and_then(Value::as_f64) {
        if n > 0.0 {
            return GuardVerdict::Tripped(format!("num_turns is {n}, expected 0"));
        }
    }
    if let Some(c) = v.get("total_cost_usd").and_then(Value::as_f64) {
        if c > 0.0 {
            return GuardVerdict::Tripped(format!("total_cost_usd is {c}, expected 0"));
        }
    }
    if let Some(m) = v.get("modelUsage").and_then(Value::as_object) {
        if !m.is_empty() {
            return GuardVerdict::Tripped(format!(
                "modelUsage is non-empty ({} entries)",
                m.len()
            ));
        }
    }

    // 4. A confirmed usage envelope whose shape is still wrong.
    if v.get("num_turns").and_then(Value::as_f64).is_none() {
        return shape("`num_turns` is absent or not numeric");
    }
    let result = match v.get("result").and_then(Value::as_str) {
        Some(r) => r,
        None => return shape("`result` is missing or not a string"),
    };

    GuardVerdict::Usage(result.to_string())
}

/// D15: the names removed from the child environment, sorted for stable logs.
/// Matching is case-insensitive: Windows environment-variable names are
/// case-insensitive to the OS but case-preserving, so a variable set as
/// `anthropic_api_key` still reaches `std::env::vars()` in that spelling and
/// must still be recognised. The original spelling is returned (not
/// upper-cased) so the caller removes the variable exactly as the OS holds
/// it.
pub fn env_names_to_strip(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = names
        .filter(|n| {
            let upper = n.to_ascii_uppercase();
            upper.starts_with("ANTHROPIC_") || upper.starts_with("CLAUDE_")
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

use chrono::{DateTime, Utc};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use super::{parser::parse_usage, PollOutcome};

/// Longest stderr / stdout tail kept in a spawn error message.
const TAIL_BYTES: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub outcome: PollOutcome,
    /// D8: kept for every outcome, success or failure.
    pub raw: Option<String>,
    pub duration_ms: u32,
}

/// Decode a child's captured pipe bytes losslessly-where-possible: invalid
/// UTF-8 becomes `U+FFFD` per byte-sequence rather than discarding the whole
/// buffer. `read_to_string` would instead fail outright on the first bad
/// byte and hand back an empty buffer, turning one stray byte into a bogus
/// unclassifiable-envelope result.
fn decode_lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn tail(s: &str) -> String {
    if s.len() <= TAIL_BYTES {
        return s.trim().to_string();
    }
    let start = s
        .char_indices()
        .map(|(i, _)| i)
        .find(|i| *i >= s.len() - TAIL_BYTES)
        .unwrap_or(0);
    s[start..].trim().to_string()
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Spawn the CLI directly — never through a shell — with the D15 sanitised
/// environment, and classify the envelope it returns.
///
/// Spec resolution: spec 6.5 sketches a `Mutex<Option<Child>>` shared with the
/// driver, but awaiting the child's exit while holding that mutex would
/// deadlock the shutdown path that wants the same mutex to kill it. The cycle
/// task therefore owns the `Child` outright and shutdown reaches it by
/// cancelling `cancel`. The child is still only ever killed through its
/// handle; `pid_slot` exists solely so the process gate can exclude it.
#[allow(clippy::too_many_arguments)]
pub async fn run_usage(
    binary: &Path,
    config_dir: &Path,
    cwd: &Path,
    timeout: Duration,
    now: DateTime<Utc>,
    pid_slot: &AtomicU32,
    cancel: &CancellationToken,
    log_env_at_info: bool,
) -> RunResult {
    let started = Instant::now();
    let timeout_secs = timeout.as_secs().clamp(1, u64::from(u32::MAX)) as u32;

    let finish = |outcome: PollOutcome, raw: Option<String>, started: Instant| RunResult {
        outcome,
        raw,
        duration_ms: started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
    };

    if let Err(e) = crate::paths::ensure_dir(cwd) {
        return finish(
            PollOutcome::SpawnError(format!("could not prepare poll cwd: {e}")),
            None,
            started,
        );
    }

    let mut cmd = Command::new(binary);
    cmd.args(USAGE_ARGV)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    // D15: strip, then set.
    let stripped = env_names_to_strip(std::env::vars().map(|(k, _)| k));
    for name in &stripped {
        cmd.env_remove(name);
    }
    cmd.env("CLAUDE_CONFIG_DIR", config_dir);
    if log_env_at_info {
        info!(stripped = ?stripped, config_dir = %config_dir.display(), "sanitised child environment");
    } else {
        debug!(stripped = ?stripped, config_dir = %config_dir.display(), "sanitised child environment");
    }

    #[cfg(windows)]
    {
        // `creation_flags` is inherent on tokio's Command, so the std
        // `CommandExt` trait must NOT be imported here: it would be an unused
        // import and `-D warnings` would reject it. (login.rs does need it,
        // because that one drives a std::process::Command.)
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return finish(
                PollOutcome::SpawnError(format!(
                    "could not spawn {}: {e}",
                    binary.display()
                )),
                None,
                started,
            )
        }
    };

    pid_slot.store(child.id().unwrap_or(0), Ordering::SeqCst);

    // Drain the pipes concurrently so a chatty child cannot fill a buffer and
    // deadlock the wait below.
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let reader = tokio::spawn(async move {
        let mut out = Vec::new();
        let mut err = Vec::new();
        if let Some(p) = stdout_pipe.as_mut() {
            let _ = p.read_to_end(&mut out).await;
        }
        if let Some(p) = stderr_pipe.as_mut() {
            let _ = p.read_to_end(&mut err).await;
        }
        (decode_lossy(&out), decode_lossy(&err))
    });

    let waited = tokio::select! {
        r = tokio::time::timeout(timeout, child.wait()) => Some(r),
        _ = cancel.cancelled() => None,
    };

    let status = match waited {
        None => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            pid_slot.store(0, Ordering::SeqCst);
            reader.abort();
            return finish(
                PollOutcome::SpawnError("cancelled during shutdown".into()),
                None,
                started,
            );
        }
        Some(Err(_elapsed)) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            pid_slot.store(0, Ordering::SeqCst);
            reader.abort();
            return finish(PollOutcome::Timeout(timeout_secs), None, started);
        }
        Some(Ok(Err(e))) => {
            pid_slot.store(0, Ordering::SeqCst);
            reader.abort();
            return finish(
                PollOutcome::SpawnError(format!("could not wait for child: {e}")),
                None,
                started,
            );
        }
        Some(Ok(Ok(s))) => s,
    };

    // The child has exited on its own (not via our kill paths above, which
    // already clear the slot). Clear it here too, once output has been
    // collected: `pid_slot` is a "this pid is one of ours, still alive"
    // exclude-hint for the process gate, and a pid that has already exited
    // can be recycled by the OS for an unrelated process, so it must not
    // linger published after run_usage has finished with it.
    let (stdout, stderr) = reader.await.unwrap_or_else(|_| (String::new(), String::new()));
    pid_slot.store(0, Ordering::SeqCst);

    if !status.success() {
        let code = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".to_string());
        let detail = if stderr.trim().is_empty() {
            tail(&stdout)
        } else {
            tail(&stderr)
        };
        return finish(
            PollOutcome::SpawnError(format!("exit {code}: {detail}")),
            Some(stdout.trim().to_string()),
            started,
        );
    }

    // D8: the raw envelope, trimmed of the trailing newline the CLI (and the
    // fake binary's `writeln!`) always emits — the stored/compared raw text
    // is the envelope itself, not incidental trailing whitespace.
    let raw = Some(stdout.trim().to_string());
    match check_envelope(&stdout) {
        // Deliberately silent. Spec 6.3 fixes the trip order as: persist the
        // halt flag, THEN log the raw envelope, then persist the outcome. If
        // this arm logged, the ERROR line would appear before the flag
        // reached disk and the log would misreport the ordering. The single
        // trip log site is `StoreHalt::log_envelope` in the driver, which
        // receives this exact stdout through `RunResult::raw`.
        GuardVerdict::Tripped(reason) => {
            finish(PollOutcome::GuardTripped(reason), raw, started)
        }
        GuardVerdict::Shape(reason) => {
            finish(PollOutcome::SpawnError(reason), raw, started)
        }
        GuardVerdict::Usage(text) => {
            debug!(result_len = text.len(), "usage envelope accepted");
            finish(parse_usage(&text, now), raw, started)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_utf8_decodes_lossily_and_keeps_the_surrounding_bytes() {
        let mut bytes = b"before-".to_vec();
        bytes.push(0xFF);
        bytes.extend_from_slice(b"-after");
        let decoded = decode_lossy(&bytes);
        assert!(decoded.contains("before-"), "{decoded:?}");
        assert!(decoded.contains("-after"), "{decoded:?}");
        assert!(
            decoded.contains('\u{FFFD}'),
            "the invalid byte must become the replacement character: {decoded:?}"
        );
    }

    fn tripped(v: &GuardVerdict) -> &str {
        match v {
            GuardVerdict::Tripped(m) => m.as_str(),
            other => panic!("expected Tripped, got {other:?}"),
        }
    }

    fn shape(v: &GuardVerdict) -> &str {
        match v {
            GuardVerdict::Shape(m) => m.as_str(),
            other => panic!("expected Shape, got {other:?}"),
        }
    }

    #[test]
    fn the_argv_constant_is_byte_for_byte_the_spec_flag_set() {
        assert_eq!(
            USAGE_ARGV,
            [
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

    #[test]
    fn a_good_usage_envelope_yields_its_result_text() {
        let v = check_envelope(
            r#"{"type":"result","local_command":"usage","num_turns":0,
                "total_cost_usd":0,"result":"Current session: 15% used"}"#,
        );
        match v {
            GuardVerdict::Usage(text) => assert_eq!(text, "Current session: 15% used"),
            other => panic!("expected Usage, got {other:?}"),
        }
    }

    // --- Rule 1: shape ---

    #[test]
    fn non_json_stdout_is_a_shape_error() {
        let v = check_envelope("this is not json");
        assert!(shape(&v).starts_with("unexpected envelope: "));
        assert!(shape(&v).contains("not JSON"));
    }

    #[test]
    fn empty_stdout_is_a_shape_error() {
        let v = check_envelope("   ");
        assert!(shape(&v).starts_with("unexpected envelope: "));
    }

    #[test]
    fn a_missing_type_field_is_a_shape_error() {
        let v = check_envelope(r#"{"local_command":"usage","num_turns":0,"result":"x"}"#);
        assert!(shape(&v).contains("no `type`"));
    }

    #[test]
    fn a_non_result_type_is_a_shape_error() {
        let v = check_envelope(r#"{"type":"system","local_command":"usage"}"#);
        assert!(shape(&v).contains("type is \"system\""));
    }

    // --- Rule 2: primary turn evidence, checked before any shape detail ---

    #[test]
    fn a_result_envelope_without_local_command_trips_the_guard() {
        let v = check_envelope(
            r#"{"type":"result","num_turns":1,"total_cost_usd":0.75,"result":"hi"}"#,
        );
        assert!(tripped(&v).contains("local_command"));
    }

    #[test]
    fn a_different_local_command_trips_the_guard() {
        let v = check_envelope(
            r#"{"type":"result","local_command":"cost","num_turns":0,"result":"x"}"#,
        );
        assert!(tripped(&v).contains("cost"));
    }

    #[test]
    fn a_malformed_turn_envelope_trips_rather_than_looking_like_a_shape_problem() {
        // Missing local_command AND missing num_turns: rule 2 runs first, so
        // this can never be misclassified as retryable.
        let v = check_envelope(r#"{"type":"result","result":"hello"}"#);
        assert!(tripped(&v).contains("local_command"));
    }

    // --- Rule 3: advisory evidence, adds trips but never excuses one ---

    #[test]
    fn a_positive_turn_count_trips_the_guard() {
        let v = check_envelope(
            r#"{"type":"result","local_command":"usage","num_turns":1,"result":"x"}"#,
        );
        assert!(tripped(&v).contains("num_turns"));
    }

    #[test]
    fn a_positive_cost_trips_the_guard_in_isolation() {
        let v = check_envelope(
            r#"{"type":"result","local_command":"usage","num_turns":0,
                "total_cost_usd":0.01,"result":"x"}"#,
        );
        assert!(tripped(&v).contains("total_cost_usd"));
    }

    #[test]
    fn a_non_empty_model_usage_map_trips_the_guard_in_isolation() {
        let v = check_envelope(
            r#"{"type":"result","local_command":"usage","num_turns":0,
                "total_cost_usd":0,"modelUsage":{"claude-fable-5-1":{"inputTokens":10}},
                "result":"x"}"#,
        );
        assert!(tripped(&v).contains("modelUsage"));
    }

    #[test]
    fn an_empty_model_usage_map_does_not_trip() {
        let v = check_envelope(
            r#"{"type":"result","local_command":"usage","num_turns":0,
                "total_cost_usd":0,"modelUsage":{},"result":"x"}"#,
        );
        assert!(matches!(v, GuardVerdict::Usage(_)));
    }

    // --- Rule 4: shape problems inside a confirmed usage envelope ---

    #[test]
    fn a_usage_envelope_missing_num_turns_is_a_shape_error() {
        let v = check_envelope(r#"{"type":"result","local_command":"usage","result":"x"}"#);
        assert!(shape(&v).contains("num_turns"));
    }

    #[test]
    fn a_usage_envelope_with_a_non_numeric_num_turns_is_a_shape_error() {
        let v = check_envelope(
            r#"{"type":"result","local_command":"usage","num_turns":"zero","result":"x"}"#,
        );
        assert!(shape(&v).contains("num_turns"));
    }

    #[test]
    fn a_usage_envelope_missing_result_is_a_shape_error() {
        let v = check_envelope(r#"{"type":"result","local_command":"usage","num_turns":0}"#);
        assert!(shape(&v).contains("result"));
    }

    // --- Rule 5: escalation lives in Machine::record (Task 12) ---

    #[test]
    fn every_shape_verdict_is_recognisable_as_a_strike_by_the_machine() {
        // The machine counts a strike by matching this prefix, so every
        // shape-class message must carry it.
        for stdout in [
            "not json",
            r#"{"local_command":"usage"}"#,
            r#"{"type":"system"}"#,
            r#"{"type":"result","local_command":"usage","result":"x"}"#,
            r#"{"type":"result","local_command":"usage","num_turns":0}"#,
        ] {
            match check_envelope(stdout) {
                GuardVerdict::Shape(m) => assert!(
                    crate::usage::is_unexpected_envelope(&m),
                    "not recognisable as a strike: {m}"
                ),
                other => panic!("expected Shape for {stdout}, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_tripped_verdict_is_never_mistaken_for_a_strike() {
        match check_envelope(r#"{"type":"result","num_turns":1,"result":"hi"}"#) {
            GuardVerdict::Tripped(m) => {
                assert!(!crate::usage::is_unexpected_envelope(&m))
            }
            other => panic!("expected Tripped, got {other:?}"),
        }
    }

    // --- D15 env sanitisation ---

    #[test]
    fn only_anthropic_and_claude_prefixed_names_are_stripped_case_insensitively() {
        // Windows environment-variable names are case-insensitive to the OS
        // but case-preserving, so a variable set as `anthropic_lowercase` or
        // `Claude_Mixed_Case` must still be recognised and stripped under
        // its original spelling.
        let names = [
            "ANTHROPIC_API_KEY",
            "CLAUDE_CONFIG_DIR",
            "CLAUDE_CODE_USE_BEDROCK",
            "PATH",
            "HOME",
            "MY_CLAUDE_THING",
            "anthropic_lowercase",
            "Claude_Mixed_Case",
        ]
        .into_iter()
        .map(|s| s.to_string());

        let stripped = env_names_to_strip(names);
        assert_eq!(
            stripped,
            vec![
                "ANTHROPIC_API_KEY".to_string(),
                "CLAUDE_CODE_USE_BEDROCK".to_string(),
                "CLAUDE_CONFIG_DIR".to_string(),
                "Claude_Mixed_Case".to_string(),
                "anthropic_lowercase".to_string(),
            ]
        );
    }
}
