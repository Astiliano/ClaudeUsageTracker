//! Test-only stand-in for the Claude Code binary. Behaviour is chosen
//! entirely by environment variables so integration tests can drive it
//! through the same spawn path the real runner uses. Excluded from the
//! shipped bundle by `mainBinaryName` in tauri.conf.json.

use std::io::Write;

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Touched ballast, so a test can give one account's child a known memory
/// peak. Held only when `CLAUDE_CONFIG_DIR` (set by the runner per account)
/// contains `FAKE_CLAUDE_BALLAST_DIR_CONTAINS`; the size is
/// `FAKE_CLAUDE_BALLAST_MB`. A non-zero fill makes the pages resident.
fn ballast() -> Vec<u8> {
    let mb: usize = env_or("FAKE_CLAUDE_BALLAST_MB", "0").parse().unwrap_or(0);
    let needle = env_or("FAKE_CLAUDE_BALLAST_DIR_CONTAINS", "");
    let dir = env_or("CLAUDE_CONFIG_DIR", "");
    if mb == 0 || needle.is_empty() || !dir.contains(&needle) {
        return Vec::new();
    }
    vec![1u8; mb * 1024 * 1024]
}

/// Minimal JSON string escaping; enough for paths, env values and argv.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn usage_envelope(result: &str) -> String {
    format!(
        r#"{{"type":"result","subtype":"success","is_error":false,"local_command":"usage","num_turns":0,"total_cost_usd":0,"duration_api_ms":0,"result":"{}"}}"#,
        json_escape(result)
    )
}

fn print_line(s: &str) {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let _ = writeln!(lock, "{s}");
    let _ = lock.flush();
}

fn main() {
    let mode = env_or("FAKE_CLAUDE_MODE", "emit");
    match mode.as_str() {
        "emit" => {
            print_line(&env_or("FAKE_CLAUDE_STDOUT", &usage_envelope("")));
        }
        "echo-argv" => {
            let args: Vec<String> = std::env::args().skip(1).collect();
            print_line(&usage_envelope(&args.join("\u{1f}")));
        }
        "echo-env" => {
            // CUT_TEST_ is an extra prefix, only ever set by our own test
            // harness, so a test can plant a sentinel that survives the
            // ANTHROPIC_/CLAUDE_ strip and prove the runner sanitises by
            // removing specific names rather than wiping the environment.
            let mut lines: Vec<String> = std::env::vars()
                .filter(|(k, _)| {
                    k.starts_with("ANTHROPIC_")
                        || k.starts_with("CLAUDE_")
                        || k.starts_with("CUT_TEST_")
                })
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            lines.sort();
            print_line(&usage_envelope(&lines.join("\n")));
        }
        "exit-nonzero" => {
            let message = env_or("FAKE_CLAUDE_STDERR", "fake failure");
            let stderr = std::io::stderr();
            let mut lock = stderr.lock();
            let _ = writeln!(lock, "{message}");
            let _ = lock.flush();
            let code: i32 = env_or("FAKE_CLAUDE_EXIT", "1").parse().unwrap_or(1);
            std::process::exit(code);
        }
        "non-json" => {
            print_line("this is not json at all");
        }
        "slow" => {
            let secs: u64 = env_or("FAKE_CLAUDE_SLEEP_SECS", "60").parse().unwrap_or(60);
            let held = ballast();
            std::thread::sleep(std::time::Duration::from_secs(secs));
            std::hint::black_box(&held);
            print_line(&usage_envelope(""));
        }
        other => {
            let stderr = std::io::stderr();
            let mut lock = stderr.lock();
            let _ = writeln!(lock, "unknown FAKE_CLAUDE_MODE: {other}");
            std::process::exit(64);
        }
    }
}
