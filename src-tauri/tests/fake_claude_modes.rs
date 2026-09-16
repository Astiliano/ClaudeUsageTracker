use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_fake_claude")
}

#[test]
fn emit_mode_writes_the_requested_stdout_and_exits_zero() {
    let out = Command::new(bin())
        .env("FAKE_CLAUDE_MODE", "emit")
        .env("FAKE_CLAUDE_STDOUT", r#"{"type":"result"}"#)
        .output()
        .expect("spawn");
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        r#"{"type":"result"}"#
    );
}

#[test]
fn echo_argv_mode_reports_every_argument_it_received() {
    let out = Command::new(bin())
        .env("FAKE_CLAUDE_MODE", "echo-argv")
        .args(["-p", "/usage", "--model", "haiku"])
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(text.trim()).expect("json");
    assert_eq!(v["type"], "result");
    assert_eq!(v["local_command"], "usage");
    let args: Vec<String> = v["result"]
        .as_str()
        .unwrap_or_default()
        .split('\u{1f}')
        .map(|s| s.to_string())
        .collect();
    assert_eq!(args, vec!["-p", "/usage", "--model", "haiku"]);
}

#[test]
fn echo_env_mode_reports_only_anthropic_and_claude_variables() {
    let out = Command::new(bin())
        .env("FAKE_CLAUDE_MODE", "echo-env")
        .env("CLAUDE_CONFIG_DIR", "/tmp/cfg")
        .env("ANTHROPIC_API_KEY", "should-be-visible-here")
        .env("UNRELATED_VAR", "ignored")
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(text.trim()).expect("json");
    let lines: Vec<&str> = v["result"].as_str().unwrap_or_default().lines().collect();
    assert!(lines.contains(&"CLAUDE_CONFIG_DIR=/tmp/cfg"));
    assert!(lines.contains(&"ANTHROPIC_API_KEY=should-be-visible-here"));
    assert!(lines.iter().all(|l| !l.starts_with("UNRELATED_VAR")));
}

#[test]
fn exit_nonzero_mode_returns_the_requested_code_and_stderr() {
    let out = Command::new(bin())
        .env("FAKE_CLAUDE_MODE", "exit-nonzero")
        .env("FAKE_CLAUDE_EXIT", "3")
        .env("FAKE_CLAUDE_STDERR", "auth failed")
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "auth failed");
}

#[test]
fn non_json_mode_exits_zero_with_junk_on_stdout() {
    let out = Command::new(bin())
        .env("FAKE_CLAUDE_MODE", "non-json")
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(serde_json::from_str::<serde_json::Value>(text.trim()).is_err());
}

#[test]
fn slow_mode_outlives_a_short_wait() {
    use std::time::Instant;
    let started = Instant::now();
    let mut child = Command::new(bin())
        .env("FAKE_CLAUDE_MODE", "slow")
        .env("FAKE_CLAUDE_SLEEP_SECS", "30")
        .spawn()
        .expect("spawn");
    // It must still be alive well inside the sleep.
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(
        child.try_wait().expect("try_wait").is_none(),
        "slow mode must not have exited yet"
    );
    child.kill().expect("kill");
    let _ = child.wait();
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}
