use std::time::Instant;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
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

/// The slice of `sysinfo::Process` both the gate and the sampler read.
/// Built once per process per refresh so the matching rule has a single
/// input shape and cannot drift between the two readers.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcView {
    pub pid: u32,
    pub parent: Option<u32>,
    /// sysinfo's seconds-since-epoch process start time; 0 when the process
    /// could not be opened.
    pub start_time: u64,
    pub name: String,
    pub cmd: Vec<String>,
}

impl From<&sysinfo::Process> for ProcView {
    fn from(p: &sysinfo::Process) -> ProcView {
        ProcView {
            pid: p.pid().as_u32(),
            parent: p.parent().map(|pp| pp.as_u32()),
            start_time: p.start_time(),
            name: p.name().to_string_lossy().to_string(),
            cmd: p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy().to_string())
                .collect(),
        }
    }
}

/// Which processes are *ours* and must not be counted (spec §4.1).
///
/// `poll_child` is the live poll child from `pid_slot`. The parent clause
/// covers the window in which the child has been spawned but `pid_slot` has
/// not been written yet. The start-time clause keeps that from misfiring:
/// Windows reports `th32ParentProcessID` even after the parent has exited,
/// so a `claude.exe` older than this app whose original parent's pid was
/// later reused by this app is still a real session and still counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exclusion {
    pub self_pid: u32,
    pub self_started_at: u64,
    pub poll_child: Option<u32>,
}

impl Exclusion {
    pub fn counts(&self, view: &ProcView) -> bool {
        if !matches_claude(&view.name, &view.cmd) {
            return false;
        }
        if Some(view.pid) == self.poll_child {
            return false;
        }
        let ours = view.parent == Some(self.self_pid) && view.start_time >= self.self_started_at;
        !ours
    }
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

/// True when any Claude Code process that is not ours is running.
pub fn is_claude_running(sys: &mut System, exclusion: &Exclusion) -> bool {
    let started = Instant::now();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_cmd(UpdateKind::OnlyIfNotSet),
    );

    let mut matched: Vec<u32> = Vec::new();
    for proc_ in sys.processes().values() {
        let view = ProcView::from(proc_);
        if exclusion.counts(&view) {
            matched.push(view.pid);
        }
    }

    debug!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        matched_pids = ?matched,
        self_pid = exclusion.self_pid,
        poll_child = ?exclusion.poll_child,
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

    fn view(pid: u32, parent: Option<u32>, start_time: u64, name: &str, cmd: &[&str]) -> ProcView {
        ProcView {
            pid,
            parent,
            start_time,
            name: name.to_string(),
            cmd: cmd.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn exclusion(poll_child: Option<u32>) -> Exclusion {
        Exclusion { self_pid: 100, self_started_at: 5_000, poll_child }
    }

    #[test]
    fn a_user_claude_counts() {
        let v = view(200, Some(1), 6_000, "claude.exe", &["claude.exe"]);
        assert!(exclusion(None).counts(&v));
    }

    #[test]
    fn an_npm_form_node_counts() {
        let v = view(
            201,
            Some(1),
            6_000,
            "node.exe",
            &["node.exe", "C:\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js"],
        );
        assert!(exclusion(None).counts(&v));
    }

    #[test]
    fn the_poll_child_is_excluded_by_pid() {
        let v = view(300, Some(100), 6_000, "claude.exe", &["claude.exe"]);
        assert!(!exclusion(Some(300)).counts(&v));
    }

    #[test]
    fn a_child_of_this_app_started_after_it_is_excluded_even_without_a_pid_slot() {
        let v = view(301, Some(100), 6_000, "claude.exe", &["claude.exe"]);
        assert!(
            !exclusion(None).counts(&v),
            "the spawn-to-pid_slot race must not let our own child latch the gate"
        );
    }

    #[test]
    fn a_session_older_than_this_app_counts_even_if_its_parent_pid_was_recycled() {
        let v = view(302, Some(100), 4_999, "claude.exe", &["claude.exe"]);
        assert!(
            exclusion(None).counts(&v),
            "Windows reports th32ParentProcessID after the parent dies; a recycled pid must not hide a real session"
        );
    }

    #[test]
    fn a_zero_self_started_at_falls_back_to_the_plain_parent_check() {
        let e = Exclusion { self_pid: 100, self_started_at: 0, poll_child: None };
        let child = view(303, Some(100), 1, "claude.exe", &["claude.exe"]);
        let other = view(304, Some(7), 1, "claude.exe", &["claude.exe"]);
        assert!(!e.counts(&child));
        assert!(e.counts(&other));
    }

    #[test]
    fn a_non_claude_process_never_counts() {
        let v = view(400, Some(1), 6_000, "code.exe", &["code.exe", "--wait"]);
        assert!(!exclusion(None).counts(&v));
    }
}
