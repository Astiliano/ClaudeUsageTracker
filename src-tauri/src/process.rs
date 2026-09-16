use std::time::Instant;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tracing::debug;

use crate::error::AppResult;

const NPM_PACKAGE_FRAGMENTS: [&str; 2] = [
    "@anthropic-ai/claude-code",
    "@anthropic-ai\\claude-code",
];

/// Pure matcher for a running Claude Code process (spec 6.2).
/// Native install: process name is `claude` or `claude.exe`.
/// npm install: name is `node`/`node.exe` and some argument carries the
/// package path fragment with either slash direction.
pub fn matches_claude(name: &str, cmd: &[String]) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower == "claude" || lower == "claude.exe" {
        return true;
    }
    if lower == "node" || lower == "node.exe" {
        return cmd
            .iter()
            .any(|a| NPM_PACKAGE_FRAGMENTS.iter().any(|f| a.contains(f)));
    }
    false
}

/// Construct the process view the gate reuses across polls.
///
/// `System::new()` is infallible in sysinfo 0.39 (it returns `Self`, not a
/// `Result`/`Option`), so this always succeeds; the `AppResult` return type
/// is kept per the interface contract for a uniform fallible construction
/// site if that ever changes upstream.
pub fn new_system() -> AppResult<System> {
    Ok(System::new())
}

/// True when any Claude Code process other than `exclude_pid` is running.
pub fn is_claude_running(sys: &mut System, exclude_pid: Option<Pid>) -> bool {
    let started = Instant::now();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_cmd(UpdateKind::OnlyIfNotSet),
    );

    let mut matched: Vec<u32> = Vec::new();
    for (pid, proc_) in sys.processes() {
        if Some(*pid) == exclude_pid {
            continue;
        }
        let name = proc_.name().to_string_lossy().to_string();
        let cmd: Vec<String> = proc_
            .cmd()
            .iter()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        if matches_claude(&name, &cmd) {
            matched.push(pid.as_u32());
        }
    }

    debug!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        matched_pids = ?matched,
        excluded_pid = ?exclude_pid.map(|p| p.as_u32()),
        "process gate check"
    );
    !matched.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn native_windows_binary_matches() {
        assert!(matches_claude(
            "claude.exe",
            &cmd(&["claude.exe", "--dangerously-skip-permissions"])
        ));
    }

    #[test]
    fn native_unix_binary_matches() {
        assert!(matches_claude("claude", &cmd(&["claude"])));
    }

    #[test]
    fn name_match_is_case_insensitive() {
        assert!(matches_claude("Claude.EXE", &cmd(&["Claude.EXE"])));
        assert!(matches_claude("CLAUDE", &cmd(&["CLAUDE"])));
    }

    #[test]
    fn npm_install_form_matches_with_forward_slashes() {
        assert!(matches_claude(
            "node",
            &cmd(&[
                "node",
                "/home/josh/.nvm/versions/node/v24.0.0/lib/node_modules/@anthropic-ai/claude-code/cli.js"
            ])
        ));
    }

    #[test]
    fn npm_install_form_matches_with_back_slashes() {
        assert!(matches_claude(
            "node.exe",
            &cmd(&[
                "node.exe",
                "C:\\Users\\josh\\AppData\\Roaming\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js"
            ])
        ));
    }

    #[test]
    fn unrelated_node_process_does_not_match() {
        assert!(!matches_claude(
            "node",
            &cmd(&["node", "/home/josh/project/server.js"])
        ));
    }

    #[test]
    fn unrelated_process_does_not_match() {
        assert!(!matches_claude("code.exe", &cmd(&["code.exe", "--wait"])));
        assert!(!matches_claude("claudette", &cmd(&["claudette"])));
        assert!(!matches_claude("myclaude.exe", &cmd(&["myclaude.exe"])));
    }

    #[test]
    fn empty_cmd_still_matches_on_the_name() {
        assert!(matches_claude("claude", &[]));
        assert!(!matches_claude("node", &[]));
    }

    #[test]
    fn the_apps_own_child_is_excluded_by_pid() {
        // matches_claude is name/cmd only; exclusion happens in
        // is_claude_running. This test documents the split so the pid rule
        // is never pushed down into the pure matcher.
        assert!(matches_claude("claude.exe", &cmd(&["claude.exe", "-p", "/usage"])));
    }
}
