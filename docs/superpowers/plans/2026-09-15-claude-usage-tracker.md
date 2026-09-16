# Claude Usage Tracker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a cross-platform Tauri 2 desktop app that shows task-manager-style Claude subscription usage (5-hour session, weekly all-models, weekly per-model) for one or more config-dir accounts, refreshing while Claude Code runs and keeping 30 days of history.

**Architecture:** A Rust backend owns everything: it spawns `claude -p /usage` directly (no shell), guards the JSON envelope against accidental model turns, parses the plain-text report, persists every outcome plus raw text to SQLite, and drives a single-cycle-at-a-time scheduler gated on a live Claude Code process. A React 18 + TypeScript webview is purely presentational: it never computes a percentage or an outcome, it refetches `get_dashboard` whenever the backend emits an event, and it renders backend DTOs plus a local 1 s tick for relative-time text.

**Tech Stack:** Rust 1.96, Tauri 2.11 (`tray-icon`), rusqlite 0.40 (bundled), sysinfo 0.39, tokio 1 + tokio-util, chrono + chrono-tz, regex, serde/serde_json, thiserror, tracing 0.1 / tracing-subscriber 0.3 / tracing-appender 0.2, dunce, dirs, uuid v4; Node 24, React 18, TypeScript, Vite, Vitest.

**Spec:** docs/2026-09-15-claude-usage-tracker-design.md

## Global Constraints

- Toolchain: Rust 1.96 (stable, MSVC on Windows), Node 24. `Cargo.lock` and `package-lock.json` are committed.
- Tauri 2.11 with the `tray-icon` feature enabled; `tauri-build` 2.6.
- Poll argv is one `const` and is byte-for-byte: `["-p", "/usage", "--output-format", "json", "--model", "haiku", "--no-session-persistence", "--strict-mcp-config", "--permission-prompts", "none", "--safe-mode"]`.
- The binary is spawned **directly** — never through `cmd.exe`, `sh`, or any shell (spec §2.2 shell hazard).
- D15 env sanitisation: inherit the parent environment, remove every variable whose name starts with `ANTHROPIC_` or `CLAUDE_`, then set `CLAUDE_CONFIG_DIR`.
- D3: on Windows only a candidate with extension `.exe` is accepted; `.cmd` / `.bat` shims are rejected with "npm shim not supported; install the native build (`claude install`) or point Settings at `claude.exe`".
- Every enum crossing a DB, log, event or command boundary serialises `snake_case`: `DisabledReason` → `user` | `guard_tripped`; `SkipReason` → `halted` | `busy` | `no_binary` | `no_enabled_accounts` | `gate_idle` | `all_backed_off`; `Trigger` → `timer` | `manual` | `startup` | `account_changed`; `Gate` → `idle` | `active`.
- Outcome strings (DB column, DTO, logs, UI, all identical): `ok`, `no_usage_data`, `parse_error`, `spawn_error`, `timeout`, `guard_tripped`. Everything except `ok` is a failure for backoff (D16) and the status pill.
- All timestamps crossing a boundary are epoch milliseconds UTC (`i64`).
- Scheduler state leaves `driver.rs` only as a `DriverStatus { gate, busy, stalled_at, backoff_until }` snapshot in `Arc<Mutex<DriverStatus>>`, published after every `decide()` and every `record()`. Nothing outside the driver holds a `Machine` handle.
- Only `interval_secs`, `timeout_secs` and `claude_binary` publish on the settings watch (moving the deadline and resetting backoff). `close_to_tray`, `launch_at_login` and `log_level` are applied directly and never touch the scheduler.
- No `unwrap()` / `expect()` in non-test code. Fallible paths return `Result<_, AppError>`; `AppError` serialises to `{code, message}`. No `any` in TypeScript.
- Gates, all four green before a task is done: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `npm test`, `npm run build`.
- npm installs use `socket npm install <pkg>` for new packages; `npm ci` for lockfile installs. Never re-enable npm scripts — the global `ignore-scripts=true` is deliberate.
- Commit after every task. Every commit message ends with exactly these two lines:

```
Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
```

## File Structure

| Path | Responsibility |
|---|---|
| `src-tauri/Cargo.toml` | Crate manifest; `[lib] name = "cut_core"`, main bin, `fake_claude` test bin, all Cargo deps |
| `src-tauri/tauri.conf.json` | Tauri config: window, bundle, `mainBinaryName`, plugin permissions |
| `src-tauri/build.rs` | `tauri_build::build()` |
| `src-tauri/capabilities/default.json` | Capability set for the main window (opener, autostart, core) |
| `src-tauri/src/main.rs` | Thin binary entry: calls `cut_core::run()` |
| `src-tauri/src/lib.rs` | Tauri builder: plugins (single-instance first, autostart, opener), tray, window close→hide, state init, scheduler start, shutdown hook |
| `src-tauri/src/error.rs` | `AppError` (thiserror) with `{code, message}` serialisation |
| `src-tauri/src/paths.rs` | home dir, app data dir, app log dir, default config dir, poll cwd, login script dir |
| `src-tauri/src/discovery.rs` | `find_claude_binary(settings)`, `enumerate_profiles(home)` |
| `src-tauri/src/process.rs` | `matches_claude(name, cmd)` (pure) and `is_claude_running(&mut System, exclude_pid)` |
| `src-tauri/src/usage/mod.rs` | `Window`, `Parsed`, `PollOutcome`, `Snapshot`, `SnapshotDto`, outcome strings |
| `src-tauri/src/usage/runner.rs` | `USAGE_ARGV`, `run_usage(...)`, envelope guard, env sanitisation, timeout |
| `src-tauri/src/usage/parser.rs` | `parse_usage(result_text, now) -> PollOutcome` (pure) |
| `src-tauri/src/scheduler/mod.rs` | Re-exports `machine` and `driver` |
| `src-tauri/src/scheduler/machine.rs` | Pure state machine: gate, busy, backoff, `decide`, `begin_cycle`, `CycleToken` |
| `src-tauri/src/scheduler/driver.rs` | tokio loop: sleep-gap deadline, settings watch, trigger notifies, watchdog, cycle task |
| `src-tauri/src/scheduler/triggers.rs` | `Notify` per trigger kind plus the accumulating account-changed id set |
| `src-tauri/src/store/mod.rs` | `Store` with `Mutex<Connection>`; every public fn sync, called via `spawn_blocking` |
| `src-tauri/src/store/schema.rs` | Pragmas and `PRAGMA user_version` migrations |
| `src-tauri/src/store/accounts.rs` | Account CRUD, seeding, rescan, canonical-path uniqueness |
| `src-tauri/src/store/snapshots.rs` | Insert, `latest_per_account`, `history`, `prune` + `incremental_vacuum`, `snapshot_raw` |
| `src-tauri/src/store/settings.rs` | Typed settings get/set, clamps, `polling_halted` |
| `src-tauri/src/commands.rs` | `#[tauri::command]` surface (spec §8) and `AppState` / `DriverStatus` |
| `src-tauri/src/tray.rs` | `tray_state(latest, halted) -> (Level, String)` (pure) plus `apply_tray` |
| `src-tauri/src/login.rs` | `open_terminal_for_login(binary, config_dir)` per OS via a written script file |
| `src-tauri/src/logging.rs` | `tracing` JSON init, daily rotation, `reload::Handle` for runtime level |
| `src-tauri/src/bin/fake_claude.rs` | Test-only helper binary: slow / echo-env / emit / exit-nonzero / non-json modes |
| `src-tauri/tests/fixtures/*.txt` | Literal `/usage` report fixtures from spec §2.2 |
| `src-tauri/tests/fake_claude_modes.rs` | Integration tests proving each `fake_claude` mode behaves as the other suites assume |
| `src-tauri/tests/runner_guard.rs` | Integration tests for the envelope guard, argv, env, timeout |
| `src-tauri/tests/driver_loop.rs` | Tokio integration tests for the driver loop |
| `src/main.tsx` | React entry point |
| `src/App.tsx` | Root component: header, accounts table, settings drawer, failure detail |
| `src/components/Header.tsx` | State banner (first match wins), Refresh now, Settings buttons |
| `src/components/AccountsTable.tsx` | One row per account: bars, per-model cells, sparkline, last-updated, pill |
| `src/components/Sparkline.tsx` | Hand-rolled SVG sparkline with gap handling |
| `src/components/Settings.tsx` | Interval, timeout, binary path, add account, rescan, toggles |
| `src/components/FailureDetail.tsx` | Modal showing stored error and raw output for a snapshot |
| `src/hooks/useDashboard.ts` | Event-driven refetch of `get_dashboard` (debounced 250 ms) + 1 s tick |
| `src/lib/types.ts` | TypeScript mirrors of the backend DTOs |
| `src/lib/format.ts` | `formatCountdown`, `formatAgo` (pure, Vitest) |
| `src/lib/sparkline.ts` | `buildSparklinePaths` (pure, Vitest) |
| `src/lib/pill.ts` | `statusPill` precedence (pure, Vitest) |
| `src/lib/banner.ts` | `bannerFor` header-state precedence (pure, Vitest) |
| `vitest.config.ts` | Vitest config: node environment, `src/**/*.test.ts` |
| `src/lib/*.test.ts` | Vitest suites for the pure helpers |
| `src/styles.css` | Dark/light via `prefers-color-scheme`; no UI library |
| `README.md` | Build instructions and the manual integration checklist (spec §10 last row) |
| `.gitignore` | Rust `target/`, node_modules, dist |

---

## Task 1: Scaffold and dependencies

**Files:** Create `.gitignore`, `package.json`, `vite.config.ts`, `tsconfig.json`, `index.html`, `src/main.tsx`, `src/App.tsx`, `src/styles.css`, `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`
**Interfaces:** Consumes: nothing. Produces: crate `cut_core` (lib) + binary `claude-usage-tracker` + test binary `fake_claude`; npm scripts `dev`, `build`, `test`, `tauri`.

- [ ] **Step 1: Scaffold the Tauri template** — run from the repo root. `create-tauri-app` v4 takes the project name positionally and `-t` / `-m` / `-y`:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
npm create tauri-app@latest -- app --template react-ts --manager npm --identifier io.zenshield.claudeusagetracker --yes
```

This writes into `./app`. Move its contents up one level and delete the empty dir:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
cp -r app/. .
rm -rf app
```

If `--yes` is rejected by the installed CTA version, re-run with `-y` in its place; if `--identifier` is rejected, drop it and edit `identifier` in `src-tauri/tauri.conf.json` by hand to `io.zenshield.claudeusagetracker`.

- [ ] **Step 2: Write `.gitignore`** — create `.gitignore` at the repo root:

```gitignore
node_modules/
dist/
src-tauri/target/
src-tauri/gen/
.DS_Store
*.log
```

- [ ] **Step 3: Write `src-tauri/Cargo.toml`** — replace the template file entirely:

```toml
[package]
name = "claude-usage-tracker"
version = "0.1.0"
description = "Task-manager style Claude subscription usage tracker"
authors = ["Josh"]
edition = "2021"
rust-version = "1.96"

[lib]
name = "cut_core"
crate-type = ["staticlib", "cdylib", "rlib"]

[[bin]]
name = "claude-usage-tracker"
path = "src/main.rs"

[[bin]]
name = "fake_claude"
path = "src/bin/fake_claude.rs"

[build-dependencies]
tauri-build = { version = "2.6", features = [] }

[dependencies]
tauri = { version = "2.11", features = ["tray-icon"] }
tauri-plugin-single-instance = "2.4"
tauri-plugin-autostart = "2.5"
tauri-plugin-opener = "2"
rusqlite = { version = "0.40", features = ["bundled"] }
sysinfo = "0.39"
tokio = { version = "1", features = ["time", "process", "sync", "macros", "rt"] }
tokio-util = "0.7"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = "0.4"
chrono-tz = "0.10"
regex = "1"
uuid = { version = "1", features = ["v4"] }
dunce = "1"
dirs = "6"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["json", "env-filter"] }
tracing-appender = "0.2"

[dev-dependencies]
tempfile = "3"
tokio = { version = "1", features = ["time", "process", "sync", "macros", "rt", "rt-multi-thread"] }
```

- [ ] **Step 4: Install the npm dependencies** — the template already pins React/Vite/TS; add Vitest and the two plugin API packages with `socket npm install` (new packages), then a lockfile install for the rest:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
socket npm install --save-dev vitest
socket npm install @tauri-apps/plugin-autostart @tauri-apps/plugin-opener
npm ci
```

Do not pass `--ignore-scripts=false`; the global `ignore-scripts=true` stays in force.

- [ ] **Step 5: Add the npm test script** — edit `package.json` so the `scripts` block reads exactly:

```json
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview",
    "test": "vitest run",
    "tauri": "tauri"
  },
```

- [ ] **Step 6: Write `src-tauri/src/main.rs`** — replace the template file:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    cut_core::run();
}
```

- [ ] **Step 7: Write a minimal `src-tauri/src/lib.rs`** — a compiling placeholder; Task 20 replaces it with the real builder:

```rust
pub mod error;

/// Entry point called by `main.rs`. Fully wired in Task 20.
pub fn run() {
    if let Err(e) = tauri::Builder::default().run(tauri::generate_context!()) {
        eprintln!("fatal: failed to run tauri application: {e}");
        std::process::exit(1);
    }
}
```

- [ ] **Step 8: Write a placeholder `src-tauri/src/error.rs`** — so `lib.rs` compiles; Task 2 writes the real one:

```rust
/// Placeholder; replaced in Task 2.
#[derive(Debug)]
pub struct AppError;
```

- [ ] **Step 9: Write a placeholder `src-tauri/src/bin/fake_claude.rs`** — so the `[[bin]]` entry resolves; Task 13 writes the real one:

```rust
fn main() {
    // Replaced in Task 13.
}
```

- [ ] **Step 10: Set the bundle's main binary and window in `src-tauri/tauri.conf.json`** — replace the file entirely. `mainBinaryName` keeps `fake_claude` out of the bundle (the bundler ships only the main binary):

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Claude Usage Tracker",
  "mainBinaryName": "claude-usage-tracker",
  "version": "0.1.0",
  "identifier": "io.zenshield.claudeusagetracker",
  "build": {
    "beforeDevCommand": "npm run dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "npm run build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "title": "Claude Usage Tracker",
        "width": 980,
        "height": 640,
        "minWidth": 720,
        "minHeight": 440,
        "resizable": true
      }
    ],
    "security": {
      "csp": "default-src 'self'; style-src 'self' 'unsafe-inline'"
    },
    "trayIcon": {
      "id": "main",
      "iconPath": "icons/icon.png",
      "iconAsTemplate": false
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

- [ ] **Step 11: Write `src-tauri/capabilities/default.json`** — replace the template file:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capabilities for the main window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:window:allow-show",
    "core:window:allow-hide",
    "core:window:allow-set-focus",
    "core:window:allow-unminimize",
    "opener:default",
    "opener:allow-open-path",
    "autostart:default"
  ]
}
```

- [ ] **Step 12: Verify the Rust side compiles** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo check --all-targets
```

Expect a clean finish. If `rusqlite` fails to link, the MSVC Build Tools C compiler is missing — install it before continuing; do not switch off `bundled`.

- [ ] **Step 13: Verify the frontend builds** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
npm run build
```

Expect `dist/` to be written with no TypeScript errors.

- [ ] **Step 14: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 1: scaffold Tauri 2 + React 18 app with pinned dependencies

Scaffolds the project from create-tauri-app (react-ts), replaces the
manifest with the spec's pinned crate set, adds the cut_core lib target
and the fake_claude test binary, and pins the bundle to the main binary
only so the test helper never ships.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 2: `AppError`

**Files:** Modify `src-tauri/src/error.rs`, `src-tauri/src/lib.rs`
**Interfaces:** Consumes: nothing. Produces: `pub enum AppError` with variants `NotFound(String)`, `Duplicate(String)`, `OutOfRange(String)`, `TerminalUnavailable(String)`, `Db(String)`, `Io(String)`, `Internal(String)`; `pub fn code(&self) -> &'static str`; `impl Serialize for AppError` emitting `{"code": "...", "message": "..."}`; `pub type AppResult<T> = Result<T, AppError>`.

- [ ] **Step 1: Write the failing test** — append to `src-tauri/src/error.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialises_to_code_and_message() {
        let e = AppError::NotFound("no such directory: /tmp/nope".into());
        let json = serde_json::to_value(&e).expect("serialise");
        assert_eq!(json["code"], "not_found");
        assert_eq!(json["message"], "no such directory: /tmp/nope");
    }

    #[test]
    fn every_variant_has_a_snake_case_code() {
        let cases = vec![
            (AppError::NotFound("a".into()), "not_found"),
            (AppError::Duplicate("a".into()), "duplicate"),
            (AppError::OutOfRange("a".into()), "out_of_range"),
            (AppError::TerminalUnavailable("a".into()), "terminal_unavailable"),
            (AppError::Db("a".into()), "db"),
            (AppError::Io("a".into()), "io"),
            (AppError::Internal("a".into()), "internal"),
        ];
        for (err, expected) in cases {
            assert_eq!(err.code(), expected);
            let json = serde_json::to_value(&err).expect("serialise");
            assert_eq!(json["code"], expected);
            assert_eq!(json["message"], "a");
        }
    }

    #[test]
    fn display_is_the_message() {
        let e = AppError::OutOfRange("interval_secs must be 10..=3600".into());
        assert_eq!(e.to_string(), "interval_secs must be 10..=3600");
    }

    #[test]
    fn rusqlite_error_maps_to_db() {
        let e: AppError = rusqlite::Error::QueryReturnedNoRows.into();
        assert_eq!(e.code(), "db");
    }

    #[test]
    fn io_error_maps_to_io() {
        let e: AppError =
            std::io::Error::new(std::io::ErrorKind::NotFound, "missing").into();
        assert_eq!(e.code(), "io");
        assert_eq!(e.to_string(), "missing");
    }
}
```

- [ ] **Step 2: Run test to verify it fails** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib error::
```

Expect compile errors: `no variant or associated item named 'NotFound' found for struct 'AppError'` and `the trait bound 'AppError: Serialize' is not satisfied`.

- [ ] **Step 3: Write minimal implementation** — replace the top of `src-tauri/src/error.rs` (everything above the `#[cfg(test)]` module) with:

```rust
use serde::ser::{Serialize, SerializeStruct, Serializer};

/// Every fallible boundary in the app returns this. It serialises to
/// `{"code": "...", "message": "..."}` exactly as spec §8 requires.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Duplicate(String),
    #[error("{0}")]
    OutOfRange(String),
    #[error("{0}")]
    TerminalUnavailable(String),
    #[error("{0}")]
    Db(String),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            AppError::NotFound(_) => "not_found",
            AppError::Duplicate(_) => "duplicate",
            AppError::OutOfRange(_) => "out_of_range",
            AppError::TerminalUnavailable(_) => "terminal_unavailable",
            AppError::Db(_) => "db",
            AppError::Io(_) => "io",
            AppError::Internal(_) => "internal",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Db(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
```

- [ ] **Step 4: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib error::
cargo clippy --all-targets -- -D warnings
```

Expect 5 passing tests and no clippy warnings.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 2: add AppError with {code, message} serialisation

Defines the single error type every command boundary returns, with
snake_case codes matching spec section 8 and From conversions for
rusqlite, io, serde_json and tauri errors.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 3: Core usage types

**Files:** Create `src-tauri/src/usage/mod.rs`; Modify `src-tauri/src/lib.rs`
**Interfaces:** Consumes: `AppError` from Task 2. Produces: `UNEXPECTED_ENVELOPE_PREFIX`, `is_unexpected_envelope(&str) -> bool`, `Window { pct: u8, resets_at: Option<i64> }`, `ModelWindow { label: String, pct: u8, resets_at: Option<i64> }`, `Parsed { session: Window, week_all: Window, week_models: Vec<(String, Window)> }`, `PollOutcome` (6 variants), `OutcomeKind` with `as_str() -> &'static str` / `from_wire(&str) -> Option<OutcomeKind>` / `is_failure() -> bool`, `PollOutcome::kind() -> OutcomeKind`, `PollOutcome::error_text() -> Option<String>` (a timeout formats `timed out after {n}s`), `Snapshot`, `SnapshotDto`, `DisabledReason`, `Account`.

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/usage/mod.rs` containing only this test module for now:

```rust
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
            is_default: false,
            created_at: 1_700_000_000_000,
        };
        let v = serde_json::to_value(&a).expect("serialise");
        assert_eq!(v["label"], "claude3");
        assert_eq!(v["enabled"], true);
        assert!(v["disabled_reason"].is_null());
        assert_eq!(v["is_default"], false);
        assert!(v["config_dir"].is_string());
    }
}
```

- [ ] **Step 2: Run test to verify it fails** — first add `pub mod usage;` to `src-tauri/src/lib.rs` under the existing `pub mod error;`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib usage::
```

Expect compile errors: `cannot find type 'OutcomeKind' in this scope`, `cannot find type 'PollOutcome' in this scope`, `cannot find type 'SnapshotDto' in this scope`.

- [ ] **Step 3: Write minimal implementation** — prepend to `src-tauri/src/usage/mod.rs`, above the test module:

```rust
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
    pub is_default: bool,
    pub created_at: i64,
}
```

- [ ] **Step 4: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib usage::
cargo clippy --all-targets -- -D warnings
```

Expect 8 passing tests, no warnings.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 3: add core usage types and snake_case wire forms

Adds Window, ModelWindow, Parsed, PollOutcome, OutcomeKind, Snapshot,
SnapshotDto, DisabledReason and Account, with the six outcome strings
used identically in the DB, DTO, logs and UI.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 4: Parser and fixtures

**Files:** Create `src-tauri/tests/fixtures/*.txt`, `src-tauri/src/usage/parser.rs`; Modify `src-tauri/src/usage/mod.rs`
**Interfaces:** Consumes: `Window`, `Parsed`, `PollOutcome` from Task 3. Produces: `pub fn parse_usage(result_text: &str, now: chrono::DateTime<chrono::Utc>) -> PollOutcome`.

- [ ] **Step 1: Write the LF fixtures** — run the block below. The separator between the percentage and the reset clause is U+00B7 MIDDLE DOT; copy the lines verbatim rather than retyping them.

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
mkdir -p tests/fixtures

cat > tests/fixtures/full_report.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current session: 15% used · resets Sep 16, 3:30am (America/Los_Angeles)
Current week (all models): 4% used · resets Sep 21, 8am (America/Los_Angeles)
Current week (Fable): 5% used · resets Sep 21, 8am (America/Los_Angeles)

What's contributing to your limits usage?
Approximate, based on local sessions on this machine — …
Last 24h · 8834 requests · 9 sessions
  99% of your usage came from subagent-heavy sessions
FIX

cat > tests/fixtures/session_zero_no_reset.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current session: 0% used
Current week (all models): 4% used · resets Sep 21, 8am (America/Los_Angeles)
FIX

cat > tests/fixtures/not_logged_in.txt <<'FIX'
Total cost: $0.0000
Total duration (API): 0ms
Total duration (wall): 0ms
Total code changes: 0 lines added, 0 lines removed
FIX

cat > tests/fixtures/two_per_model.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current session: 15% used · resets Sep 16, 3:30am (America/Los_Angeles)
Current week (all models): 4% used · resets Sep 21, 8am (America/Los_Angeles)
Current week (Fable): 5% used · resets Sep 21, 8am (America/Los_Angeles)
Current week (Opus): 12% used · resets Sep 21, 8am (America/Los_Angeles)
FIX

cat > tests/fixtures/no_per_model.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current session: 15% used · resets Sep 16, 3:30am (America/Los_Angeles)
Current week (all models): 4% used · resets Sep 21, 8am (America/Los_Angeles)
FIX

cat > tests/fixtures/noon_midnight.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current session: 3% used · resets Sep 16, 12am (America/Los_Angeles)
Current week (all models): 9% used · resets Sep 21, 12pm (America/Los_Angeles)
FIX

cat > tests/fixtures/dec_to_jan.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current session: 22% used · resets Dec 29, 5am (America/Los_Angeles)
Current week (all models): 61% used · resets Jan 3, 8am (America/Los_Angeles)
FIX

cat > tests/fixtures/missing_session.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current week (all models): 4% used · resets Sep 21, 8am (America/Los_Angeles)
FIX

cat > tests/fixtures/missing_week_all.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current session: 15% used · resets Sep 16, 3:30am (America/Los_Angeles)
FIX

cat > tests/fixtures/pct_out_of_range.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current session: 150% used · resets Sep 16, 3:30am (America/Los_Angeles)
Current week (all models): 4% used · resets Sep 21, 8am (America/Los_Angeles)
FIX

cat > tests/fixtures/non_numeric_pct.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current session: many% used · resets Sep 16, 3:30am (America/Los_Angeles)
Current week (all models): 4% used · resets Sep 21, 8am (America/Los_Angeles)
FIX

cat > tests/fixtures/unknown_zone.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current session: 15% used · resets Sep 16, 3:30am (Mars/Olympus_Mons)
Current week (all models): 4% used · resets Sep 21, 8am (Mars/Olympus_Mons)
FIX

cat > tests/fixtures/dst_gap.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current session: 11% used · resets Mar 8, 2:30am (America/New_York)
Current week (all models): 33% used · resets Mar 12, 8am (America/New_York)
FIX

cat > tests/fixtures/dst_ambiguous.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current session: 11% used · resets Nov 1, 1:30am (America/New_York)
Current week (all models): 33% used · resets Nov 5, 8am (America/New_York)
FIX

cat > tests/fixtures/unknown_extra_lines.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current session: 15% used · resets Sep 16, 3:30am (America/Los_Angeles)
Current quarter (experimental): 77% used · resets Dec 1, 9am (America/Los_Angeles)
Current week (all models): 4% used · resets Sep 21, 8am (America/Los_Angeles)
Brand new line the CLI added in a future version
FIX

cat > tests/fixtures/duplicate_session.txt <<'FIX'
You are currently using your subscription to power your Claude Code usage

Current session: 15% used · resets Sep 16, 3:30am (America/Los_Angeles)
Current session: 16% used · resets Sep 16, 3:30am (America/Los_Angeles)
Current week (all models): 4% used · resets Sep 21, 8am (America/Los_Angeles)
FIX
```

- [ ] **Step 2: Write the CRLF fixture** — a heredoc would strip the carriage returns, so build this one with `printf`. Each literal percent sign is doubled for `printf`; the middle dot is typed literally:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
printf 'You are currently using your subscription to power your Claude Code usage\r\n\r\nCurrent session: 15%% used · resets Sep 16, 3:30am (America/Los_Angeles)\r\nCurrent week (all models): 4%% used · resets Sep 21, 8am (America/Los_Angeles)\r\n' > tests/fixtures/crlf.txt
```

- [ ] **Step 3: Verify the CRLF fixture really has carriage returns and middle dots** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
od -c tests/fixtures/crlf.txt | grep -c '\\r'
grep -c '·' tests/fixtures/crlf.txt
grep -c '15% used' tests/fixtures/crlf.txt
```

Expect a non-zero number from each of the three commands. If `15%% used` landed in the file literally, rewrite that line with a single percent sign.

- [ ] **Step 4: Write the failing test** — create `src-tauri/src/usage/parser.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<chrono::Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0)
            .single()
            .expect("fixed test instant must be valid")
    }

    fn ms(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        at(y, mo, d, h, mi).timestamp_millis()
    }

    fn now_sep_2026() -> chrono::DateTime<chrono::Utc> {
        at(2026, 9, 15, 20, 0)
    }

    fn parsed(text: &str, now: chrono::DateTime<chrono::Utc>) -> Parsed {
        match parse_usage(text, now) {
            PollOutcome::Ok(p) => p,
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn full_report_parses_all_three_lines() {
        let p = parsed(
            include_str!("../../tests/fixtures/full_report.txt"),
            now_sep_2026(),
        );
        assert_eq!(p.session.pct, 15);
        // Sep 16 2026 03:30 America/Los_Angeles (PDT, UTC-7) == 10:30 UTC.
        assert_eq!(p.session.resets_at, Some(ms(2026, 9, 16, 10, 30)));
        assert_eq!(p.week_all.pct, 4);
        // Sep 21 2026 08:00 PDT == 15:00 UTC.
        assert_eq!(p.week_all.resets_at, Some(ms(2026, 9, 21, 15, 0)));
        assert_eq!(p.week_models.len(), 1);
        assert_eq!(p.week_models[0].0, "Fable");
        assert_eq!(p.week_models[0].1.pct, 5);
        assert_eq!(p.week_models[0].1.resets_at, Some(ms(2026, 9, 21, 15, 0)));
    }

    #[test]
    fn zero_percent_session_without_a_reset_clause() {
        let p = parsed(
            include_str!("../../tests/fixtures/session_zero_no_reset.txt"),
            now_sep_2026(),
        );
        assert_eq!(p.session.pct, 0);
        assert_eq!(p.session.resets_at, None);
        assert_eq!(p.week_all.pct, 4);
    }

    #[test]
    fn not_logged_in_cost_summary_is_no_usage_data() {
        let out = parse_usage(
            include_str!("../../tests/fixtures/not_logged_in.txt"),
            now_sep_2026(),
        );
        assert_eq!(out, PollOutcome::NoUsageData);
    }

    #[test]
    fn two_per_model_lines_keep_output_order() {
        let p = parsed(
            include_str!("../../tests/fixtures/two_per_model.txt"),
            now_sep_2026(),
        );
        assert_eq!(p.week_models.len(), 2);
        assert_eq!(p.week_models[0].0, "Fable");
        assert_eq!(p.week_models[0].1.pct, 5);
        assert_eq!(p.week_models[1].0, "Opus");
        assert_eq!(p.week_models[1].1.pct, 12);
    }

    #[test]
    fn zero_per_model_lines_is_fine() {
        let p = parsed(
            include_str!("../../tests/fixtures/no_per_model.txt"),
            now_sep_2026(),
        );
        assert!(p.week_models.is_empty());
    }

    #[test]
    fn r3_never_steals_the_all_models_line() {
        let p = parsed(
            include_str!("../../tests/fixtures/full_report.txt"),
            now_sep_2026(),
        );
        assert!(
            p.week_models.iter().all(|(l, _)| l != "all models"),
            "the all-models line must never appear in week_models"
        );
    }

    #[test]
    fn missing_session_line_is_a_parse_error() {
        let out = parse_usage(
            include_str!("../../tests/fixtures/missing_session.txt"),
            now_sep_2026(),
        );
        assert_eq!(out, PollOutcome::ParseError("missing session line".into()));
    }

    #[test]
    fn missing_week_all_line_is_a_parse_error() {
        let out = parse_usage(
            include_str!("../../tests/fixtures/missing_week_all.txt"),
            now_sep_2026(),
        );
        assert_eq!(
            out,
            PollOutcome::ParseError("missing week (all models) line".into())
        );
    }

    #[test]
    fn pct_over_one_hundred_is_a_parse_error() {
        let out = parse_usage(
            include_str!("../../tests/fixtures/pct_out_of_range.txt"),
            now_sep_2026(),
        );
        assert_eq!(out, PollOutcome::ParseError("pct out of range".into()));
    }

    #[test]
    fn non_numeric_pct_fails_the_line_and_then_the_report() {
        // The line matches no regex, so it is ignored for forward
        // compatibility; the resulting missing required line is what surfaces.
        let out = parse_usage(
            include_str!("../../tests/fixtures/non_numeric_pct.txt"),
            now_sep_2026(),
        );
        assert_eq!(out, PollOutcome::ParseError("missing session line".into()));
    }

    #[test]
    fn duplicate_session_line_is_a_parse_error() {
        let out = parse_usage(
            include_str!("../../tests/fixtures/duplicate_session.txt"),
            now_sep_2026(),
        );
        assert_eq!(out, PollOutcome::ParseError("duplicate session line".into()));
    }

    #[test]
    fn unknown_zone_is_a_parse_error() {
        let out = parse_usage(
            include_str!("../../tests/fixtures/unknown_zone.txt"),
            now_sep_2026(),
        );
        assert_eq!(
            out,
            PollOutcome::ParseError("unknown zone: Mars/Olympus_Mons".into())
        );
    }

    #[test]
    fn unknown_extra_lines_are_ignored() {
        let p = parsed(
            include_str!("../../tests/fixtures/unknown_extra_lines.txt"),
            now_sep_2026(),
        );
        assert_eq!(p.session.pct, 15);
        assert_eq!(p.week_all.pct, 4);
        assert!(p.week_models.is_empty());
    }

    #[test]
    fn crlf_input_parses() {
        let p = parsed(include_str!("../../tests/fixtures/crlf.txt"), now_sep_2026());
        assert_eq!(p.session.pct, 15);
        assert_eq!(p.week_all.pct, 4);
        assert_eq!(p.session.resets_at, Some(ms(2026, 9, 16, 10, 30)));
    }

    #[test]
    fn twelve_am_is_midnight_and_twelve_pm_is_noon() {
        let p = parsed(
            include_str!("../../tests/fixtures/noon_midnight.txt"),
            now_sep_2026(),
        );
        // Sep 16 2026 00:00 PDT == Sep 16 07:00 UTC.
        assert_eq!(p.session.resets_at, Some(ms(2026, 9, 16, 7, 0)));
        // Sep 21 2026 12:00 PDT == Sep 21 19:00 UTC.
        assert_eq!(p.week_all.resets_at, Some(ms(2026, 9, 21, 19, 0)));
    }

    #[test]
    fn december_to_january_wraps_the_year() {
        let now = at(2026, 12, 28, 18, 0);
        let p = parsed(include_str!("../../tests/fixtures/dec_to_jan.txt"), now);
        // Dec 29 2026 05:00 PST (UTC-8) == Dec 29 13:00 UTC, same year.
        assert_eq!(p.session.resets_at, Some(ms(2026, 12, 29, 13, 0)));
        // Jan 3 08:00 PST would be 2026 under the naive rule, far more than
        // 30 days in the past, so it rolls to 2027: Jan 3 16:00 UTC.
        assert_eq!(p.week_all.resets_at, Some(ms(2027, 1, 3, 16, 0)));
    }

    #[test]
    fn dst_gap_resolves_to_the_first_valid_instant_after_the_gap() {
        let now = at(2026, 3, 7, 12, 0);
        let p = parsed(include_str!("../../tests/fixtures/dst_gap.txt"), now);
        // 2026-03-08 02:30 America/New_York does not exist; the first valid
        // local instant after the gap is 03:00 EDT == 07:00 UTC.
        assert_eq!(p.session.resets_at, Some(ms(2026, 3, 8, 7, 0)));
    }

    #[test]
    fn ambiguous_local_time_resolves_to_the_earliest() {
        let now = at(2026, 10, 31, 12, 0);
        let p = parsed(include_str!("../../tests/fixtures/dst_ambiguous.txt"), now);
        // 2026-11-01 01:30 America/New_York happens twice; the earliest is
        // EDT (UTC-4) == 05:30 UTC.
        assert_eq!(p.session.resets_at, Some(ms(2026, 11, 1, 5, 30)));
    }

    #[test]
    fn empty_text_is_no_usage_data() {
        assert_eq!(parse_usage("", now_sep_2026()), PollOutcome::NoUsageData);
    }
}
```

- [ ] **Step 5: Run test to verify it fails** — first add `pub mod parser;` as the first line of `src-tauri/src/usage/mod.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib usage::parser::
```

Expect `cannot find function 'parse_usage' in this scope` and `cannot find type 'Parsed' in this scope`.

- [ ] **Step 6: Write minimal implementation** — prepend to `src-tauri/src/usage/parser.rs`, above the test module:

```rust
use chrono::{DateTime, Datelike, Duration, LocalResult, TimeZone, Utc};
use chrono_tz::Tz;
use regex::Regex;
use std::sync::OnceLock;

use super::{Parsed, PollOutcome, Window};

struct Patterns {
    detect: Regex,
    session: Regex,
    week_all: Regex,
    week_model: Regex,
    clause: Regex,
}

/// Compiled once. Returns `None` only if a literal pattern fails to compile,
/// which the caller turns into a parse error rather than a panic.
fn patterns() -> Option<&'static Patterns> {
    static P: OnceLock<Option<Patterns>> = OnceLock::new();
    P.get_or_init(|| {
        Some(Patterns {
            detect: Regex::new(r"^Current (session|week)\b").ok()?,
            session: Regex::new(
                r"^Current session: (\d{1,3})% used(?: · resets (.+?) \((.+)\))?$",
            )
            .ok()?,
            week_all: Regex::new(
                r"^Current week \(all models\): (\d{1,3})% used(?: · resets (.+?) \((.+)\))?$",
            )
            .ok()?,
            week_model: Regex::new(
                r"^Current week \((.+?)\): (\d{1,3})% used(?: · resets (.+?) \((.+)\))?$",
            )
            .ok()?,
            clause: Regex::new(r"^([A-Z][a-z]{2}) (\d{1,2}), (\d{1,2})(?::(\d{2}))?(am|pm)$")
                .ok()?,
        })
    })
    .as_ref()
}

fn month_from_abbrev(a: &str) -> Option<u32> {
    match a {
        "Jan" => Some(1),
        "Feb" => Some(2),
        "Mar" => Some(3),
        "Apr" => Some(4),
        "May" => Some(5),
        "Jun" => Some(6),
        "Jul" => Some(7),
        "Aug" => Some(8),
        "Sep" => Some(9),
        "Oct" => Some(10),
        "Nov" => Some(11),
        "Dec" => Some(12),
        _ => None,
    }
}

/// Resolve a wall-clock local time in `tz` to a UTC instant.
/// DST gap: first valid instant after the gap. Ambiguous: earliest.
fn resolve_local(tz: Tz, y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Option<DateTime<Utc>> {
    match tz.with_ymd_and_hms(y, mo, d, h, mi, 0) {
        LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(earliest, _latest) => Some(earliest.with_timezone(&Utc)),
        LocalResult::None => {
            let start = h * 60 + mi;
            for step in 1..=180u32 {
                let total = start + step;
                if total >= 24 * 60 {
                    return None;
                }
                match tz.with_ymd_and_hms(y, mo, d, total / 60, total % 60, 0) {
                    LocalResult::Single(dt) => return Some(dt.with_timezone(&Utc)),
                    LocalResult::Ambiguous(earliest, _) => {
                        return Some(earliest.with_timezone(&Utc))
                    }
                    LocalResult::None => continue,
                }
            }
            None
        }
    }
}

/// Parse `Sep 16, 3:30am` plus `America/Los_Angeles` into epoch ms UTC.
fn parse_reset(p: &Patterns, clause: &str, zone: &str, now: DateTime<Utc>) -> Result<i64, String> {
    let c = p
        .clause
        .captures(clause)
        .ok_or_else(|| format!("bad reset clause: {clause}"))?;
    let month = month_from_abbrev(&c[1]).ok_or_else(|| format!("bad month: {}", &c[1]))?;
    let day: u32 = c[2].parse().map_err(|_| format!("bad day: {}", &c[2]))?;
    let hour12: u32 = c[3].parse().map_err(|_| format!("bad hour: {}", &c[3]))?;
    if !(1..=12).contains(&hour12) {
        return Err(format!("hour out of range: {hour12}"));
    }
    let minute: u32 = match c.get(4) {
        Some(m) => m
            .as_str()
            .parse()
            .map_err(|_| format!("bad minute: {}", m.as_str()))?,
        None => 0,
    };
    if minute > 59 {
        return Err(format!("minute out of range: {minute}"));
    }
    let hour = match (hour12, &c[5]) {
        (12, "am") => 0,
        (12, _) => 12,
        (h, "pm") => h + 12,
        (h, _) => h,
    };

    let tz: Tz = zone.parse().map_err(|_| format!("unknown zone: {zone}"))?;
    let year = now.with_timezone(&tz).year();
    let first = resolve_local(tz, year, month, day, hour, minute)
        .ok_or_else(|| format!("invalid local time: {clause} ({zone})"))?;
    if first < now - Duration::days(30) {
        let next = resolve_local(tz, year + 1, month, day, hour, minute)
            .ok_or_else(|| format!("invalid local time: {clause} ({zone})"))?;
        return Ok(next.timestamp_millis());
    }
    Ok(first.timestamp_millis())
}

fn pct_from(raw: &str) -> Result<u8, String> {
    let n: u16 = raw.parse().map_err(|_| format!("bad pct: {raw}"))?;
    if n > 100 {
        return Err("pct out of range".to_string());
    }
    Ok(n as u8)
}

/// Build a `Window` from a pct capture plus the optional reset captures.
fn window_from(
    p: &Patterns,
    pct_raw: &str,
    clause: Option<&str>,
    zone: Option<&str>,
    now: DateTime<Utc>,
) -> Result<Window, String> {
    let pct = pct_from(pct_raw)?;
    let resets_at = match (clause, zone) {
        (Some(c), Some(z)) => Some(parse_reset(p, c, z, now)?),
        _ => None,
    };
    Ok(Window { pct, resets_at })
}

/// Parse the `/usage` report text. Pure: every clock input arrives as `now`.
pub fn parse_usage(result_text: &str, now: DateTime<Utc>) -> PollOutcome {
    let p = match patterns() {
        Some(p) => p,
        None => return PollOutcome::ParseError("internal: regex compilation failed".into()),
    };

    let lines: Vec<&str> = result_text.lines().map(|l| l.trim_end()).collect();

    if !lines.iter().any(|l| p.detect.is_match(l)) {
        return PollOutcome::NoUsageData;
    }

    let mut session: Option<Window> = None;
    let mut week_all: Option<Window> = None;
    let mut week_models: Vec<(String, Window)> = Vec::new();

    for line in &lines {
        if let Some(c) = p.session.captures(line) {
            if session.is_some() {
                return PollOutcome::ParseError("duplicate session line".into());
            }
            match window_from(
                p,
                &c[1],
                c.get(2).map(|m| m.as_str()),
                c.get(3).map(|m| m.as_str()),
                now,
            ) {
                Ok(w) => session = Some(w),
                Err(e) => return PollOutcome::ParseError(e),
            }
            continue;
        }
        if let Some(c) = p.week_all.captures(line) {
            if week_all.is_some() {
                return PollOutcome::ParseError("duplicate week (all models) line".into());
            }
            match window_from(
                p,
                &c[1],
                c.get(2).map(|m| m.as_str()),
                c.get(3).map(|m| m.as_str()),
                now,
            ) {
                Ok(w) => week_all = Some(w),
                Err(e) => return PollOutcome::ParseError(e),
            }
            continue;
        }
        if let Some(c) = p.week_model.captures(line) {
            let label = c[1].to_string();
            match window_from(
                p,
                &c[2],
                c.get(3).map(|m| m.as_str()),
                c.get(4).map(|m| m.as_str()),
                now,
            ) {
                Ok(w) => week_models.push((label, w)),
                Err(e) => return PollOutcome::ParseError(e),
            }
            continue;
        }
        // Unknown line: ignored for forward compatibility.
    }

    let session = match session {
        Some(w) => w,
        None => return PollOutcome::ParseError("missing session line".into()),
    };
    let week_all = match week_all {
        Some(w) => w,
        None => return PollOutcome::ParseError("missing week (all models) line".into()),
    };

    PollOutcome::Ok(Parsed {
        session,
        week_all,
        week_models,
    })
}
```

- [ ] **Step 7: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib usage::parser::
cargo clippy --all-targets -- -D warnings
```

Expect 19 passing tests, no warnings.

- [ ] **Step 8: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 4: add the /usage text parser with the full fixture corpus

Implements the ordered session / week-all / week-model line match, the
reset-clause grammar with chrono-tz resolution (DST gap forward, ambiguous
earliest, December-to-January year wrap) and the content-based detection
predicate. Fixtures cover every parser row of the testing plan, including
CRLF, 12am/12pm and the not-logged-in cost summary.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 5: Process gate matcher

**Files:** Create `src-tauri/src/process.rs`; Modify `src-tauri/src/lib.rs`
**Interfaces:** Consumes: nothing. Produces: `pub fn matches_claude(name: &str, cmd: &[String]) -> bool`, `pub fn is_claude_running(sys: &mut sysinfo::System, exclude_pid: Option<sysinfo::Pid>) -> bool`, `pub fn new_system() -> Result<sysinfo::System, AppError>`.

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/process.rs` containing only this test module for now:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails** — first add `pub mod process;` to `src-tauri/src/lib.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib process::
```

Expect `cannot find function 'matches_claude' in this scope`.

- [ ] **Step 3: Write minimal implementation** — prepend to `src-tauri/src/process.rs`, above the test module:

```rust
use std::time::Instant;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tracing::debug;

use crate::error::{AppError, AppResult};

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
pub fn new_system() -> AppResult<System> {
    System::new().map_err(|e| AppError::Internal(format!("sysinfo init failed: {e}")))
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
```

- [ ] **Step 4: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib process::
cargo clippy --all-targets -- -D warnings
```

Expect 9 passing tests, no warnings.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 5: add the Claude Code process gate matcher

Adds the pure matches_claude predicate covering the native binary and the
npm cli.js form with either slash direction, plus is_claude_running which
refreshes only exe and cmd, honours exclude_pid for the app's own child,
and logs timing and matched pids at DEBUG.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 6: Paths

**Files:** Create `src-tauri/src/paths.rs`; Modify `src-tauri/src/lib.rs`
**Interfaces:** Consumes: `AppError`, `AppResult` from Task 2. Produces: `pub fn home_dir() -> AppResult<PathBuf>`, `pub fn default_config_dir(env_override: Option<&str>, home: &Path) -> PathBuf`, `pub fn db_path(app_data_dir: &Path) -> PathBuf`, `pub fn poll_cwd(app_data_dir: &Path) -> PathBuf`, `pub fn login_script_dir(app_data_dir: &Path) -> PathBuf`, `pub fn ensure_dir(dir: &Path) -> AppResult<()>`, `pub fn empty_dir(dir: &Path) -> AppResult<()>`.

Note on ownership: the Tauri-provided `app_data_dir` and `app_log_dir` are read once in Task 20 via `app.path()` and passed into these functions, so this module stays unit-testable without a Tauri runtime.

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/paths.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_dir_prefers_the_env_override() {
        let home = Path::new("/home/josh");
        assert_eq!(
            default_config_dir(Some("/home/josh/.claude3"), home),
            PathBuf::from("/home/josh/.claude3")
        );
    }

    #[test]
    fn default_config_dir_falls_back_to_dot_claude() {
        let home = Path::new("/home/josh");
        assert_eq!(
            default_config_dir(None, home),
            PathBuf::from("/home/josh/.claude")
        );
    }

    #[test]
    fn blank_env_override_is_treated_as_unset() {
        let home = Path::new("/home/josh");
        assert_eq!(
            default_config_dir(Some("   "), home),
            PathBuf::from("/home/josh/.claude")
        );
        assert_eq!(
            default_config_dir(Some(""), home),
            PathBuf::from("/home/josh/.claude")
        );
    }

    #[test]
    fn derived_paths_hang_off_the_app_data_dir() {
        let base = Path::new("/data/cut");
        assert_eq!(db_path(base), PathBuf::from("/data/cut/usage.sqlite"));
        assert_eq!(poll_cwd(base), PathBuf::from("/data/cut/poll-cwd"));
        assert_eq!(login_script_dir(base), PathBuf::from("/data/cut/login"));
    }

    #[test]
    fn ensure_dir_creates_nested_directories_and_is_idempotent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("a").join("b").join("c");
        ensure_dir(&target).expect("first create");
        assert!(target.is_dir());
        ensure_dir(&target).expect("second create is a no-op");
        assert!(target.is_dir());
    }

    #[test]
    fn empty_dir_removes_contents_but_keeps_the_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("login");
        ensure_dir(&target).expect("create");
        std::fs::write(target.join("login.cmd"), "old").expect("write file");
        std::fs::create_dir(target.join("sub")).expect("create sub");
        std::fs::write(target.join("sub").join("x"), "old").expect("write nested");

        empty_dir(&target).expect("empty");

        assert!(target.is_dir());
        let left = std::fs::read_dir(&target)
            .expect("read_dir")
            .count();
        assert_eq!(left, 0);
    }

    #[test]
    fn empty_dir_creates_the_directory_when_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("nope");
        empty_dir(&target).expect("empty");
        assert!(target.is_dir());
    }
}
```

- [ ] **Step 2: Run test to verify it fails** — first add `pub mod paths;` to `src-tauri/src/lib.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib paths::
```

Expect `cannot find function 'default_config_dir' in this scope`.

- [ ] **Step 3: Write minimal implementation** — prepend to `src-tauri/src/paths.rs`, above the test module:

```rust
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// The user's home directory.
pub fn home_dir() -> AppResult<PathBuf> {
    dirs::home_dir().ok_or_else(|| AppError::NotFound("home directory not found".into()))
}

/// D4: the default account is `CLAUDE_CONFIG_DIR` from the app's own
/// environment if set and non-blank, else `<home>/.claude`.
pub fn default_config_dir(env_override: Option<&str>, home: &Path) -> PathBuf {
    match env_override.map(str::trim) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home.join(".claude"),
    }
}

pub fn db_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("usage.sqlite")
}

/// The working directory every poll child is spawned in (spec 6.3).
pub fn poll_cwd(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("poll-cwd")
}

/// Where the login helper script is written (spec 6.8).
pub fn login_script_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("login")
}

pub fn ensure_dir(dir: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dir).map_err(|e| {
        AppError::Io(format!("could not create {}: {e}", dir.display()))
    })
}

/// Remove everything inside `dir`, creating `dir` if it does not exist.
pub fn empty_dir(dir: &Path) -> AppResult<()> {
    if !dir.exists() {
        return ensure_dir(dir);
    }
    let entries = std::fs::read_dir(dir)
        .map_err(|e| AppError::Io(format!("could not read {}: {e}", dir.display())))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| AppError::Io(format!("could not read {}: {e}", dir.display())))?;
        let path = entry.path();
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        result.map_err(|e| AppError::Io(format!("could not remove {}: {e}", path.display())))?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib paths::
cargo clippy --all-targets -- -D warnings
```

Expect 7 passing tests, no warnings.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 6: add path helpers for config dir, database, poll cwd and login scripts

Keeps every derived path a pure function of an injected app data dir so the
module is unit-testable without a Tauri runtime, and adds ensure_dir and
empty_dir used by the poll cwd and the login script directory.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 7: Discovery — binary and profiles

**Files:** Create `src-tauri/src/discovery.rs`; Modify `src-tauri/src/lib.rs`
**Interfaces:** Consumes: `AppResult` from Task 2, `paths::home_dir` from Task 6. Produces: `pub enum BinarySource { Override, LocalBin, Path }` with `as_str()`, `pub struct Found { path: PathBuf, source: BinarySource }`, `pub const CMD_SHIM_MESSAGE: &str`, `pub fn accept_candidate(path: &Path, is_windows: bool, is_file: bool, unix_mode: Option<u32>) -> bool`, `pub fn find_claude_binary_in(override_path: Option<&str>, home: &Path, path_dirs: &[PathBuf]) -> Option<Found>`, `pub fn find_claude_binary(override_path: Option<&str>) -> Option<Found>`, `pub struct Candidate { config_dir: PathBuf, label: String }`, `pub fn enumerate_profiles(home: &Path) -> Vec<Candidate>`.

- [ ] **Step 1: Write the failing test for binary acceptance** — create `src-tauri/src/discovery.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn windows_accepts_only_exe() {
        assert!(accept_candidate(
            Path::new("C:/Users/josh/.local/bin/claude.exe"),
            true,
            true,
            None
        ));
        assert!(accept_candidate(
            Path::new("C:/Users/josh/.local/bin/CLAUDE.EXE"),
            true,
            true,
            None
        ));
    }

    #[test]
    fn windows_rejects_cmd_and_bat_shims() {
        assert!(!accept_candidate(
            Path::new("C:/Users/josh/AppData/Roaming/npm/claude.cmd"),
            true,
            true,
            None
        ));
        assert!(!accept_candidate(
            Path::new("C:/Users/josh/AppData/Roaming/npm/claude.bat"),
            true,
            true,
            None
        ));
        assert!(!accept_candidate(
            Path::new("C:/Users/josh/AppData/Roaming/npm/claude"),
            true,
            true,
            None
        ));
    }

    #[test]
    fn windows_rejects_a_directory() {
        assert!(!accept_candidate(
            Path::new("C:/Users/josh/.local/bin/claude.exe"),
            true,
            false,
            None
        ));
    }

    #[test]
    fn unix_requires_an_executable_mode_bit() {
        assert!(accept_candidate(
            Path::new("/home/josh/.local/bin/claude"),
            false,
            true,
            Some(0o755)
        ));
        assert!(!accept_candidate(
            Path::new("/home/josh/.local/bin/claude"),
            false,
            true,
            Some(0o644)
        ));
        assert!(!accept_candidate(
            Path::new("/home/josh/.local/bin/claude"),
            false,
            false,
            Some(0o755)
        ));
    }

    #[test]
    fn the_shim_message_is_the_spec_wording() {
        assert_eq!(
            CMD_SHIM_MESSAGE,
            "npm shim not supported; install the native build (`claude install`) or point Settings at `claude.exe`"
        );
    }

    #[test]
    fn binary_source_wire_forms_are_snake_case() {
        assert_eq!(BinarySource::Override.as_str(), "override");
        assert_eq!(BinarySource::LocalBin.as_str(), "local_bin");
        assert_eq!(BinarySource::Path.as_str(), "path");
    }

    /// Create a file that `accept_candidate` will accept on this platform.
    fn write_executable(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, b"#!/bin/sh\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }
    }

    fn binary_name() -> &'static str {
        if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        }
    }

    #[test]
    fn override_wins_over_local_bin_and_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let over = tmp.path().join("custom").join(binary_name());
        let local = home.join(".local").join("bin").join(binary_name());
        let on_path_dir = tmp.path().join("pathdir");
        let on_path = on_path_dir.join(binary_name());
        write_executable(&over);
        write_executable(&local);
        write_executable(&on_path);

        let found = find_claude_binary_in(
            Some(&over.to_string_lossy()),
            &home,
            &[on_path_dir.clone()],
        )
        .expect("found");
        assert_eq!(found.source, BinarySource::Override);
        assert_eq!(found.path, over);
    }

    #[test]
    fn local_bin_wins_over_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let local = home.join(".local").join("bin").join(binary_name());
        let on_path_dir = tmp.path().join("pathdir");
        write_executable(&local);
        write_executable(&on_path_dir.join(binary_name()));

        let found = find_claude_binary_in(None, &home, &[on_path_dir]).expect("found");
        assert_eq!(found.source, BinarySource::LocalBin);
        assert_eq!(found.path, local);
    }

    #[test]
    fn falls_back_to_the_first_path_hit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        let first = tmp.path().join("p1");
        let second = tmp.path().join("p2");
        write_executable(&first.join(binary_name()));
        write_executable(&second.join(binary_name()));

        let found = find_claude_binary_in(None, &home, &[first.clone(), second]).expect("found");
        assert_eq!(found.source, BinarySource::Path);
        assert_eq!(found.path, first.join(binary_name()));
    }

    #[test]
    fn a_blank_override_is_ignored() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        let local = home.join(".local").join("bin").join(binary_name());
        write_executable(&local);

        let found = find_claude_binary_in(Some("  "), &home, &[]).expect("found");
        assert_eq!(found.source, BinarySource::LocalBin);
    }

    #[test]
    fn nothing_found_returns_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        assert!(find_claude_binary_in(None, &home, &[]).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn a_cmd_shim_on_path_is_skipped_entirely() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        let shim_dir = tmp.path().join("npm");
        write_executable(&shim_dir.join("claude.cmd"));
        assert!(find_claude_binary_in(None, &home, &[shim_dir]).is_none());
    }

    /// Build the spec 2.3 home layout in a temp dir.
    fn profile_home() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();

        let dot_claude = home.join(".claude");
        fs::create_dir_all(dot_claude.join("projects")).expect("mk .claude");
        fs::write(dot_claude.join(".credentials.json"), "{}").expect("w");
        fs::write(dot_claude.join("settings.json"), "{}").expect("w");

        for name in [".claude2", ".claude3"] {
            let d = home.join(name);
            fs::create_dir_all(&d).expect("mk");
            fs::write(d.join(".credentials.json"), "{}").expect("w");
            fs::write(d.join(".claude.json"), "{}").expect("w");
            fs::write(d.join("settings.json"), "{}").expect("w");
        }

        for name in [".claude-free", ".claude-kilofree"] {
            let d = home.join(name);
            fs::create_dir_all(d.join("projects")).expect("mk");
            fs::write(d.join(".claude.json"), "{}").expect("w");
        }

        let flow = home.join(".claude-flow");
        fs::create_dir_all(&flow).expect("mk");
        fs::write(flow.join("update-state.json"), "{}").expect("w");

        // A non-matching directory and a matching-looking plain file.
        fs::create_dir_all(home.join(".config")).expect("mk");
        fs::write(home.join(".claude.json"), "{}").expect("w");

        tmp
    }

    #[test]
    fn enumerate_profiles_finds_exactly_the_config_dirs() {
        let tmp = profile_home();
        let found = enumerate_profiles(tmp.path());
        let labels: Vec<&str> = found.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["claude", "claude-free", "claude-kilofree", "claude2", "claude3"]
        );
    }

    #[test]
    fn enumerate_profiles_excludes_the_update_state_only_dir() {
        let tmp = profile_home();
        let found = enumerate_profiles(tmp.path());
        assert!(
            found.iter().all(|c| c.label != "claude-flow"),
            "a dir with only update-state.json is not a config dir"
        );
    }

    #[test]
    fn enumerate_profiles_excludes_plain_files() {
        let tmp = profile_home();
        let found = enumerate_profiles(tmp.path());
        assert!(found.iter().all(|c| c.config_dir.is_dir()));
    }

    #[test]
    fn enumerate_profiles_canonicalises_without_a_verbatim_prefix() {
        let tmp = profile_home();
        let found = enumerate_profiles(tmp.path());
        for c in &found {
            let s = c.config_dir.to_string_lossy().to_string();
            assert!(!s.starts_with(r"\\?\"), "unexpected verbatim prefix: {s}");
        }
    }

    #[test]
    fn enumerate_profiles_is_sorted_case_insensitively_by_label() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for name in [".claudeZ", ".claudea", ".claudeB"] {
            let d = tmp.path().join(name);
            fs::create_dir_all(&d).expect("mk");
            fs::write(d.join("settings.json"), "{}").expect("w");
        }
        let found = enumerate_profiles(tmp.path());
        let labels: Vec<&str> = found.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["claudea", "claudeB", "claudeZ"]);
    }

    #[test]
    fn enumerate_profiles_on_a_missing_home_returns_empty() {
        let missing = PathBuf::from("/definitely/not/a/home/dir/here");
        assert!(enumerate_profiles(&missing).is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails** — first add `pub mod discovery;` to `src-tauri/src/lib.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib discovery::
```

Expect `cannot find function 'accept_candidate' in this scope` and `cannot find type 'BinarySource' in this scope`.

- [ ] **Step 3: Write minimal implementation** — prepend to `src-tauri/src/discovery.rs`, above the test module:

```rust
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// D3: the exact message shown when a Windows `.cmd` / `.bat` shim is found.
pub const CMD_SHIM_MESSAGE: &str =
    "npm shim not supported; install the native build (`claude install`) or point Settings at `claude.exe`";

const CONFIG_DIR_MARKERS: [&str; 3] = [".credentials.json", ".claude.json", "settings.json"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinarySource {
    Override,
    LocalBin,
    Path,
}

impl BinarySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            BinarySource::Override => "override",
            BinarySource::LocalBin => "local_bin",
            BinarySource::Path => "path",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub source: BinarySource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub config_dir: PathBuf,
    pub label: String,
}

/// D3 acceptance, expressed purely so it can be tested on any host:
/// a regular file, and on Windows the extension must be exactly `exe`
/// (case-insensitively); elsewhere an executable mode bit must be set.
pub fn accept_candidate(
    path: &Path,
    is_windows: bool,
    is_file: bool,
    unix_mode: Option<u32>,
) -> bool {
    if !is_file {
        return false;
    }
    if is_windows {
        return matches!(
            path.extension().and_then(|e| e.to_str()),
            Some(ext) if ext.eq_ignore_ascii_case("exe")
        );
    }
    matches!(unix_mode, Some(m) if m & 0o111 != 0)
}

/// Apply `accept_candidate` to a real path, and log the D3 skip for a shim.
fn is_acceptable_binary(path: &Path) -> bool {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode())
    };
    #[cfg(not(unix))]
    let mode: Option<u32> = None;

    let accepted = accept_candidate(path, cfg!(windows), meta.is_file(), mode);
    if !accepted && meta.is_file() && cfg!(windows) {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat") {
                warn!(path = %path.display(), "{}", CMD_SHIM_MESSAGE);
            }
        }
    }
    accepted
}

fn binary_file_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["claude.exe", "claude.cmd", "claude.bat", "claude"]
    } else {
        &["claude"]
    }
}

/// Settings override, then `<home>/.local/bin/claude[.exe]`, then the first
/// acceptable hit walking `path_dirs` in order (spec 6.1).
pub fn find_claude_binary_in(
    override_path: Option<&str>,
    home: &Path,
    path_dirs: &[PathBuf],
) -> Option<Found> {
    if let Some(raw) = override_path.map(str::trim) {
        if !raw.is_empty() {
            let p = PathBuf::from(raw);
            if is_acceptable_binary(&p) {
                info!(path = %p.display(), source = "override", "claude binary selected");
                return Some(Found {
                    path: p,
                    source: BinarySource::Override,
                });
            }
        }
    }

    let local_bin = home.join(".local").join("bin");
    for name in binary_file_names() {
        let p = local_bin.join(name);
        if is_acceptable_binary(&p) {
            info!(path = %p.display(), source = "local_bin", "claude binary selected");
            return Some(Found {
                path: p,
                source: BinarySource::LocalBin,
            });
        }
    }

    for dir in path_dirs {
        for name in binary_file_names() {
            let p = dir.join(name);
            if is_acceptable_binary(&p) {
                info!(path = %p.display(), source = "path", "claude binary selected");
                return Some(Found {
                    path: p,
                    source: BinarySource::Path,
                });
            }
        }
    }

    None
}

/// Production wrapper: real home directory and the real `PATH`.
pub fn find_claude_binary(override_path: Option<&str>) -> Option<Found> {
    let home = crate::paths::home_dir().ok()?;
    let path_dirs: Vec<PathBuf> = match std::env::var_os("PATH") {
        Some(p) => std::env::split_paths(&p).collect(),
        None => Vec::new(),
    };
    find_claude_binary_in(override_path, &home, &path_dirs)
}

/// Every `<home>/.claude*` directory carrying at least one config marker,
/// canonicalised with `dunce` so Windows paths have no verbatim prefix.
/// Sorted case-insensitively by label; default-first ordering (D17) is
/// applied by the store when it lists accounts.
pub fn enumerate_profiles(home: &Path) -> Vec<Candidate> {
    let entries = match std::fs::read_dir(home) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut out: Vec<Candidate> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !name.starts_with(".claude") {
            continue;
        }
        if !CONFIG_DIR_MARKERS.iter().any(|m| path.join(m).is_file()) {
            continue;
        }
        let config_dir = dunce::canonicalize(&path).unwrap_or(path);
        let label = name.trim_start_matches('.').to_string();
        out.push(Candidate { config_dir, label });
    }

    out.sort_by(|a, b| {
        a.label
            .to_lowercase()
            .cmp(&b.label.to_lowercase())
            .then_with(|| a.label.cmp(&b.label))
    });
    out
}
```

- [ ] **Step 4: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib discovery::
cargo clippy --all-targets -- -D warnings
```

Expect 18 passing tests on Windows (17 elsewhere; the `.cmd` PATH test is Windows-only), no warnings.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 7: add binary discovery and config-dir profile enumeration

Implements the override then local-bin then PATH precedence with the D3
rule that only a real .exe is accepted on Windows, logging the npm shim
skip at WARN, and enumerates dot-claude profile directories by marker file
with dunce canonicalisation and case-insensitive label ordering.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 8: Store — connection, pragmas and migrations

**Files:** Create `src-tauri/src/store/mod.rs`, `src-tauri/src/store/schema.rs`; Modify `src-tauri/src/lib.rs`
**Interfaces:** Consumes: `AppError`, `AppResult` from Task 2. Produces: `pub struct Store`, `Store::open(path: &Path) -> AppResult<Store>`, `Store::open_in_memory() -> AppResult<Store>`, `Store::with_conn<T>(&self, f: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<T>`, `schema::SCHEMA_VERSION: i64`, `schema::apply_pragmas(&Connection) -> AppResult<()>`, `schema::migrate(&Connection) -> AppResult<()>`.

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/store/schema.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn table_names(store: &Store) -> Vec<String> {
        store
            .with_conn(|c| {
                let mut stmt = c.prepare(
                    "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
                )?;
                let rows = stmt
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .expect("query tables")
    }

    #[test]
    fn migrating_an_empty_database_creates_every_table() {
        let store = Store::open_in_memory().expect("open");
        let names = table_names(&store);
        assert!(names.contains(&"accounts".to_string()));
        assert!(names.contains(&"settings".to_string()));
        assert!(names.contains(&"snapshots".to_string()));
    }

    #[test]
    fn user_version_is_set_to_the_schema_version() {
        let store = Store::open_in_memory().expect("open");
        let v: i64 = store
            .with_conn(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?))
            .expect("read user_version");
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn migrating_twice_is_a_no_op() {
        let store = Store::open_in_memory().expect("open");
        store
            .with_conn(|c| migrate(c))
            .expect("second migrate must succeed");
        let v: i64 = store
            .with_conn(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?))
            .expect("read user_version");
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn the_expected_indexes_exist() {
        let store = Store::open_in_memory().expect("open");
        let names: Vec<String> = store
            .with_conn(|c| {
                let mut stmt = c.prepare(
                    "SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'snapshots%'",
                )?;
                let rows = stmt
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .expect("query indexes");
        assert!(names.contains(&"snapshots_acct_time".to_string()));
        assert!(names.contains(&"snapshots_time".to_string()));
    }

    #[test]
    fn foreign_keys_are_on() {
        let store = Store::open_in_memory().expect("open");
        let on: i64 = store
            .with_conn(|c| Ok(c.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?))
            .expect("read pragma");
        assert_eq!(on, 1);
    }

    #[test]
    fn auto_vacuum_is_incremental() {
        let store = Store::open_in_memory().expect("open");
        // 0 = NONE, 1 = FULL, 2 = INCREMENTAL
        let mode: i64 = store
            .with_conn(|c| Ok(c.query_row("PRAGMA auto_vacuum", [], |r| r.get(0))?))
            .expect("read pragma");
        assert_eq!(mode, 2);
    }

    #[test]
    fn a_file_backed_store_uses_wal() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open(&tmp.path().join("usage.sqlite")).expect("open");
        let mode: String = store
            .with_conn(|c| Ok(c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?))
            .expect("read pragma");
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn reopening_a_file_store_keeps_its_data() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("usage.sqlite");
        {
            let store = Store::open(&path).expect("open");
            store
                .with_conn(|c| {
                    c.execute(
                        "INSERT INTO settings(key, value) VALUES('probe','yes')",
                        [],
                    )?;
                    Ok(())
                })
                .expect("insert");
        }
        let store = Store::open(&path).expect("reopen");
        let v: String = store
            .with_conn(|c| {
                Ok(c.query_row("SELECT value FROM settings WHERE key='probe'", [], |r| {
                    r.get(0)
                })?)
            })
            .expect("read back");
        assert_eq!(v, "yes");
    }
}
```

- [ ] **Step 2: Run test to verify it fails** — first create `src-tauri/src/store/mod.rs` with the single line `pub mod schema;`, add `pub mod store;` to `src-tauri/src/lib.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib store::
```

Expect `cannot find type 'Store' in 'crate::store'` and `cannot find value 'SCHEMA_VERSION' in this scope`.

- [ ] **Step 3: Write the schema implementation** — prepend to `src-tauri/src/store/schema.rs`, above the test module:

```rust
use rusqlite::Connection;

use crate::error::AppResult;

pub const SCHEMA_VERSION: i64 = 1;

/// Spec 6.6. `auto_vacuum` is set first because SQLite only honours a change
/// while the database is still empty.
pub fn apply_pragmas(conn: &Connection) -> AppResult<()> {
    conn.execute_batch("PRAGMA auto_vacuum=INCREMENTAL;")?;
    // journal_mode returns a row, so it cannot go through execute_batch.
    let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
    Ok(())
}

const V1: &str = r#"
CREATE TABLE accounts(
  id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  config_dir TEXT NOT NULL UNIQUE,
  enabled INTEGER NOT NULL,
  disabled_reason TEXT,
  is_default INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE settings(
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE snapshots(
  id INTEGER PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  taken_at INTEGER NOT NULL,
  outcome TEXT NOT NULL,
  session_pct INTEGER,
  session_resets_at INTEGER,
  week_all_pct INTEGER,
  week_all_resets_at INTEGER,
  week_models TEXT,
  error TEXT,
  raw TEXT,
  duration_ms INTEGER NOT NULL
);
CREATE INDEX snapshots_acct_time ON snapshots(account_id, taken_at DESC, id DESC);
CREATE INDEX snapshots_time ON snapshots(taken_at);
"#;

/// Migrate forward using `PRAGMA user_version`. Idempotent.
pub fn migrate(conn: &Connection) -> AppResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current < 1 {
        conn.execute_batch(V1)?;
    }
    if current < SCHEMA_VERSION {
        conn.execute_batch(&format!("PRAGMA user_version={SCHEMA_VERSION};"))?;
    }
    Ok(())
}
```

- [ ] **Step 4: Write the store implementation** — replace `src-tauri/src/store/mod.rs` with:

```rust
pub mod schema;

use rusqlite::Connection;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::error::AppResult;

/// Spec 6.6: a single `Mutex<Connection>`. Every public method here is
/// synchronous; callers wrap them in `spawn_blocking` so the mutex is never
/// held across an `await`.
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> AppResult<Store> {
        if let Some(parent) = path.parent() {
            crate::paths::ensure_dir(parent)?;
        }
        let conn = Connection::open(path)?;
        schema::apply_pragmas(&conn)?;
        schema::migrate(&conn)?;
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    /// Tests only in practice, but not gated on `cfg(test)` so integration
    /// tests can use it too.
    pub fn open_in_memory() -> AppResult<Store> {
        let conn = Connection::open_in_memory()?;
        schema::apply_pragmas(&conn)?;
        schema::migrate(&conn)?;
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    /// A poisoned mutex means a previous caller panicked while holding it.
    /// The connection itself is still usable, so recover rather than panic.
    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<T> {
        let guard = self.lock();
        f(&guard)
    }

    pub fn with_conn_mut<T>(
        &self,
        f: impl FnOnce(&mut Connection) -> AppResult<T>,
    ) -> AppResult<T> {
        let mut guard = self.lock();
        f(&mut guard)
    }
}
```

- [ ] **Step 5: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib store::
cargo clippy --all-targets -- -D warnings
```

Expect 8 passing tests, no warnings.

- [ ] **Step 6: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 8: add the SQLite store with pragmas and version-gated migrations

Opens the database with auto_vacuum INCREMENTAL set before any table is
created, then WAL, foreign_keys and a 5 s busy timeout, and creates the
accounts, settings and snapshots tables plus both snapshot indexes behind
PRAGMA user_version. The connection mutex recovers from poisoning rather
than panicking.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 9: Store — settings

**Files:** Create `src-tauri/src/store/settings.rs`; Modify `src-tauri/src/store/mod.rs`
**Interfaces:** Consumes: `Store` from Task 8. Produces: `pub struct UserSettings { interval_secs: u32, timeout_secs: u32, claude_binary: String, close_to_tray: bool, launch_at_login: bool, log_level: String }`, constants `MIN_INTERVAL_SECS`/`MAX_INTERVAL_SECS`/`DEFAULT_INTERVAL_SECS`/`MIN_TIMEOUT_SECS`/`MAX_TIMEOUT_SECS`/`DEFAULT_TIMEOUT_SECS`, `pub fn validate_settings(&UserSettings) -> AppResult<()>`, `pub fn polling_relevant_changed(&UserSettings, &UserSettings) -> bool`, `Store::get_raw`, `Store::set_raw`, `Store::stored_settings() -> AppResult<UserSettings>`, `Store::save_settings(&UserSettings) -> AppResult<()>`, `Store::polling_halted() -> AppResult<Option<String>>`, `Store::set_polling_halted(&str)`, `Store::clear_polling_halted() -> AppResult<Option<String>>`.

`launch_at_login` is carried on `UserSettings` for the wire but is never persisted: `stored_settings` always returns `false` for it and `save_settings` ignores it. The command layer in Task 18 reads and writes the autostart plugin's live state instead (spec 6.6).

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/store/settings.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn defaults() -> UserSettings {
        UserSettings {
            interval_secs: DEFAULT_INTERVAL_SECS,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            claude_binary: String::new(),
            close_to_tray: true,
            launch_at_login: false,
            log_level: "info".to_string(),
        }
    }

    #[test]
    fn a_fresh_store_returns_the_documented_defaults() {
        let store = Store::open_in_memory().expect("open");
        let s = store.stored_settings().expect("read");
        assert_eq!(s.interval_secs, 60);
        assert_eq!(s.timeout_secs, 30);
        assert_eq!(s.claude_binary, "");
        assert!(s.close_to_tray);
        assert!(!s.launch_at_login);
        assert_eq!(s.log_level, "info");
    }

    #[test]
    fn settings_round_trip() {
        let store = Store::open_in_memory().expect("open");
        let mut s = defaults();
        s.interval_secs = 120;
        s.timeout_secs = 45;
        s.claude_binary = "C:/bin/claude.exe".into();
        s.close_to_tray = false;
        s.log_level = "debug".into();
        store.save_settings(&s).expect("save");

        let back = store.stored_settings().expect("read");
        assert_eq!(back.interval_secs, 120);
        assert_eq!(back.timeout_secs, 45);
        assert_eq!(back.claude_binary, "C:/bin/claude.exe");
        assert!(!back.close_to_tray);
        assert_eq!(back.log_level, "debug");
    }

    #[test]
    fn boundary_values_are_accepted() {
        for v in [MIN_INTERVAL_SECS, MAX_INTERVAL_SECS] {
            let mut s = defaults();
            s.interval_secs = v;
            validate_settings(&s).unwrap_or_else(|e| panic!("{v} must be valid: {e}"));
        }
        for v in [MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS] {
            let mut s = defaults();
            s.timeout_secs = v;
            validate_settings(&s).unwrap_or_else(|e| panic!("{v} must be valid: {e}"));
        }
    }

    #[test]
    fn one_off_values_are_rejected_as_out_of_range() {
        for v in [MIN_INTERVAL_SECS - 1, MAX_INTERVAL_SECS + 1] {
            let mut s = defaults();
            s.interval_secs = v;
            let err = validate_settings(&s).expect_err("must reject");
            assert_eq!(err.code(), "out_of_range");
        }
        for v in [MIN_TIMEOUT_SECS - 1, MAX_TIMEOUT_SECS + 1] {
            let mut s = defaults();
            s.timeout_secs = v;
            let err = validate_settings(&s).expect_err("must reject");
            assert_eq!(err.code(), "out_of_range");
        }
    }

    #[test]
    fn an_unknown_log_level_is_out_of_range() {
        let mut s = defaults();
        s.log_level = "trace".into();
        let err = validate_settings(&s).expect_err("must reject");
        assert_eq!(err.code(), "out_of_range");
    }

    #[test]
    fn save_settings_rejects_out_of_range_input_before_writing() {
        let store = Store::open_in_memory().expect("open");
        let mut s = defaults();
        s.interval_secs = 9;
        let err = store.save_settings(&s).expect_err("must reject");
        assert_eq!(err.code(), "out_of_range");
        assert_eq!(store.stored_settings().expect("read").interval_secs, 60);
    }

    #[test]
    fn launch_at_login_is_never_persisted() {
        let store = Store::open_in_memory().expect("open");
        let mut s = defaults();
        s.launch_at_login = true;
        store.save_settings(&s).expect("save");
        assert!(!store.stored_settings().expect("read").launch_at_login);
        assert_eq!(
            store.get_raw("launch_at_login").expect("read raw"),
            None,
            "launch_at_login must not reach the settings table"
        );
    }

    #[test]
    fn polling_halted_starts_absent_and_round_trips() {
        let store = Store::open_in_memory().expect("open");
        assert_eq!(store.polling_halted().expect("read"), None);
        store
            .set_polling_halted("guard_tripped:1700000000000")
            .expect("set");
        assert_eq!(
            store.polling_halted().expect("read"),
            Some("guard_tripped:1700000000000".to_string())
        );
    }

    #[test]
    fn clear_polling_halted_returns_the_previous_value() {
        let store = Store::open_in_memory().expect("open");
        store
            .set_polling_halted("guard_tripped:1700000000000")
            .expect("set");
        let prev = store.clear_polling_halted().expect("clear");
        assert_eq!(prev, Some("guard_tripped:1700000000000".to_string()));
        assert_eq!(store.polling_halted().expect("read"), None);
        assert_eq!(store.clear_polling_halted().expect("clear again"), None);
    }

    #[test]
    fn polling_halted_survives_close_and_reopen() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("usage.sqlite");
        {
            let store = Store::open(&path).expect("open");
            store
                .set_polling_halted("guard_tripped:1700000000000")
                .expect("set");
        }
        let store = Store::open(&path).expect("reopen");
        assert_eq!(
            store.polling_halted().expect("read"),
            Some("guard_tripped:1700000000000".to_string())
        );
    }

    #[test]
    fn saving_user_settings_does_not_touch_polling_halted() {
        let store = Store::open_in_memory().expect("open");
        store
            .set_polling_halted("guard_tripped:1700000000000")
            .expect("set");
        store.save_settings(&defaults()).expect("save");
        assert_eq!(
            store.polling_halted().expect("read"),
            Some("guard_tripped:1700000000000".to_string())
        );
    }

    #[test]
    fn only_the_three_polling_keys_are_scheduler_relevant() {
        let base = defaults();

        let mut interval = base.clone();
        interval.interval_secs = 120;
        assert!(polling_relevant_changed(&base, &interval));

        let mut timeout = base.clone();
        timeout.timeout_secs = 45;
        assert!(polling_relevant_changed(&base, &timeout));

        let mut binary = base.clone();
        binary.claude_binary = "C:/bin/claude.exe".into();
        assert!(polling_relevant_changed(&base, &binary));
    }

    #[test]
    fn the_other_three_keys_never_touch_the_scheduler() {
        let base = defaults();

        let mut tray = base.clone();
        tray.close_to_tray = false;
        assert!(!polling_relevant_changed(&base, &tray));

        let mut autostart = base.clone();
        autostart.launch_at_login = true;
        assert!(!polling_relevant_changed(&base, &autostart));

        let mut level = base.clone();
        level.log_level = "debug".into();
        assert!(!polling_relevant_changed(&base, &level));
    }

    #[test]
    fn an_identical_save_is_not_a_change() {
        assert!(!polling_relevant_changed(&defaults(), &defaults()));
    }
}
```

- [ ] **Step 2: Run test to verify it fails** — first add `pub mod settings;` and `pub use settings::{validate_settings, UserSettings};` to `src-tauri/src/store/mod.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib store::settings::
```

Expect `cannot find type 'UserSettings' in this scope` and `no method named 'stored_settings' found for struct 'Store'`.

- [ ] **Step 3: Write minimal implementation** — prepend to `src-tauri/src/store/settings.rs`, above the test module:

```rust
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::store::Store;

pub const MIN_INTERVAL_SECS: u32 = 10;
pub const MAX_INTERVAL_SECS: u32 = 3600;
pub const DEFAULT_INTERVAL_SECS: u32 = 60;
pub const MIN_TIMEOUT_SECS: u32 = 5;
pub const MAX_TIMEOUT_SECS: u32 = 120;
pub const DEFAULT_TIMEOUT_SECS: u32 = 30;

pub const KEY_POLLING_HALTED: &str = "polling_halted";

/// The user-facing settings struct carried by `get_settings` / `set_settings`.
/// `launch_at_login` is on the wire but never in the table: the command layer
/// reads and writes the autostart plugin's live state (spec 6.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSettings {
    pub interval_secs: u32,
    pub timeout_secs: u32,
    pub claude_binary: String,
    pub close_to_tray: bool,
    pub launch_at_login: bool,
    pub log_level: String,
}

/// D5 and spec 6.3/7 clamps. Rejection code is `out_of_range`.
pub fn validate_settings(s: &UserSettings) -> AppResult<()> {
    if !(MIN_INTERVAL_SECS..=MAX_INTERVAL_SECS).contains(&s.interval_secs) {
        return Err(AppError::OutOfRange(format!(
            "interval_secs must be {MIN_INTERVAL_SECS}..={MAX_INTERVAL_SECS}, got {}",
            s.interval_secs
        )));
    }
    if !(MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(&s.timeout_secs) {
        return Err(AppError::OutOfRange(format!(
            "timeout_secs must be {MIN_TIMEOUT_SECS}..={MAX_TIMEOUT_SECS}, got {}",
            s.timeout_secs
        )));
    }
    if s.log_level != "info" && s.log_level != "debug" {
        return Err(AppError::OutOfRange(format!(
            "log_level must be info or debug, got {}",
            s.log_level
        )));
    }
    Ok(())
}

/// D16 / spec 8: only `interval_secs`, `timeout_secs` and `claude_binary`
/// are polling-relevant. A change to one of them publishes on the settings
/// watch, which moves the deadline and resets backoff. `close_to_tray`,
/// `launch_at_login` and `log_level` are applied directly and must never
/// touch the scheduler.
pub fn polling_relevant_changed(previous: &UserSettings, next: &UserSettings) -> bool {
    previous.interval_secs != next.interval_secs
        || previous.timeout_secs != next.timeout_secs
        || previous.claude_binary != next.claude_binary
}

impl Store {
    pub fn get_raw(&self, key: &str) -> AppResult<Option<String>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT value FROM settings WHERE key = ?1")?;
            let mut rows = stmt.query([key])?;
            match rows.next()? {
                Some(row) => Ok(Some(row.get::<_, String>(0)?)),
                None => Ok(None),
            }
        })
    }

    pub fn set_raw(&self, key: &str, value: &str) -> AppResult<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO settings(key, value) VALUES(?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![key, value],
            )?;
            Ok(())
        })
    }

    fn get_u32(&self, key: &str, default: u32) -> AppResult<u32> {
        Ok(self
            .get_raw(key)?
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(default))
    }

    fn get_bool(&self, key: &str, default: bool) -> AppResult<bool> {
        Ok(self
            .get_raw(key)?
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(default))
    }

    /// Everything the table knows. `launch_at_login` is always `false` here.
    pub fn stored_settings(&self) -> AppResult<UserSettings> {
        Ok(UserSettings {
            interval_secs: self.get_u32("interval_secs", DEFAULT_INTERVAL_SECS)?,
            timeout_secs: self.get_u32("timeout_secs", DEFAULT_TIMEOUT_SECS)?,
            claude_binary: self.get_raw("claude_binary")?.unwrap_or_default(),
            close_to_tray: self.get_bool("close_to_tray", true)?,
            launch_at_login: false,
            log_level: self
                .get_raw("log_level")?
                .unwrap_or_else(|| "info".to_string()),
        })
    }

    /// Validates first, so an out-of-range value never reaches the table.
    pub fn save_settings(&self, s: &UserSettings) -> AppResult<()> {
        validate_settings(s)?;
        self.set_raw("interval_secs", &s.interval_secs.to_string())?;
        self.set_raw("timeout_secs", &s.timeout_secs.to_string())?;
        self.set_raw("claude_binary", &s.claude_binary)?;
        self.set_raw("close_to_tray", if s.close_to_tray { "1" } else { "0" })?;
        self.set_raw("log_level", &s.log_level)?;
        Ok(())
    }

    /// `None`, or `guard_tripped:<epoch ms>`. Survives restarts by design.
    pub fn polling_halted(&self) -> AppResult<Option<String>> {
        self.get_raw(KEY_POLLING_HALTED)
    }

    pub fn set_polling_halted(&self, value: &str) -> AppResult<()> {
        self.set_raw(KEY_POLLING_HALTED, value)
    }

    /// Clears the flag and returns whatever it held.
    pub fn clear_polling_halted(&self) -> AppResult<Option<String>> {
        let previous = self.polling_halted()?;
        self.with_conn(|c| {
            c.execute(
                "DELETE FROM settings WHERE key = ?1",
                rusqlite::params![KEY_POLLING_HALTED],
            )?;
            Ok(())
        })?;
        Ok(previous)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib store::settings::
cargo clippy --all-targets -- -D warnings
```

Expect 14 passing tests, no warnings.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 9: add typed settings storage with clamps and the halt flag

Stores interval, timeout, binary override, close-to-tray and log level as
key/value rows, validates the D5 and timeout clamps before writing, keeps
launch_at_login off disk, and persists polling_halted separately so a
settings save can never clear it.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 10: Store — accounts

**Files:** Create `src-tauri/src/store/accounts.rs`; Modify `src-tauri/src/store/mod.rs`
**Interfaces:** Consumes: `Store` (Task 8), `Account` / `DisabledReason` (Task 3), `discovery::Candidate` (Task 7). Produces: `Store::list_accounts() -> AppResult<Vec<Account>>`, `Store::enabled_account_ids() -> AppResult<Vec<String>>`, `Store::account_by_id(&str) -> AppResult<Option<Account>>`, `Store::add_account(&Path, bool, Option<DisabledReason>, bool, i64) -> AppResult<Account>`, `Store::update_account(&str, Option<&str>, Option<bool>) -> AppResult<Account>`, `Store::remove_account(&str) -> AppResult<()>`, `Store::mark_guard_tripped(&str) -> AppResult<()>`, `Store::seed_accounts_if_empty(&[Candidate], &Path, i64) -> AppResult<usize>`, `Store::rescan_accounts(&[Candidate], i64) -> AppResult<Vec<Account>>`.

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/store/accounts.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::Candidate;
    use crate::store::Store;
    use std::path::PathBuf;

    const NOW: i64 = 1_700_000_000_000;

    fn candidate(dir: &std::path::Path, label: &str) -> Candidate {
        Candidate {
            config_dir: dir.to_path_buf(),
            label: label.to_string(),
        }
    }

    fn make_dir(root: &std::path::Path, name: &str) -> PathBuf {
        let d = root.join(name);
        std::fs::create_dir_all(&d).expect("mkdir");
        std::fs::write(d.join("settings.json"), "{}").expect("marker");
        dunce::canonicalize(&d).unwrap_or(d)
    }

    #[test]
    fn add_account_stores_and_returns_the_row() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let dir = make_dir(tmp.path(), ".claude3");

        let a = store
            .add_account(&dir, true, None, false, NOW)
            .expect("add");
        assert_eq!(a.label, "claude3");
        assert!(a.enabled);
        assert_eq!(a.disabled_reason, None);
        assert!(!a.is_default);
        assert_eq!(a.created_at, NOW);
        assert_eq!(a.config_dir, dir);
        assert_eq!(a.id.len(), 36, "uuid v4 hyphenated");
    }

    #[test]
    fn add_account_rejects_a_missing_directory() {
        let store = Store::open_in_memory().expect("open");
        let err = store
            .add_account(
                &PathBuf::from("/definitely/not/here/.claude9"),
                true,
                None,
                false,
                NOW,
            )
            .expect_err("must reject");
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn add_account_rejects_a_duplicate_canonical_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let dir = make_dir(tmp.path(), ".claude3");
        store.add_account(&dir, true, None, false, NOW).expect("add");
        let err = store
            .add_account(&dir, true, None, false, NOW)
            .expect_err("must reject");
        assert_eq!(err.code(), "duplicate");
    }

    #[test]
    fn list_accounts_puts_the_default_first_then_labels_case_insensitively() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let zed = make_dir(tmp.path(), ".claudeZed");
        let alpha = make_dir(tmp.path(), ".claudealpha");
        let def = make_dir(tmp.path(), ".claudeMain");

        store.add_account(&zed, true, None, false, NOW).expect("a");
        store.add_account(&alpha, true, None, false, NOW).expect("b");
        store.add_account(&def, true, None, true, NOW).expect("c");

        let labels: Vec<String> = store
            .list_accounts()
            .expect("list")
            .into_iter()
            .map(|a| a.label)
            .collect();
        assert_eq!(labels, vec!["claudeMain", "claudealpha", "claudeZed"]);
    }

    #[test]
    fn disabling_an_account_sets_the_user_reason_and_enabling_clears_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let dir = make_dir(tmp.path(), ".claude3");
        let a = store.add_account(&dir, true, None, false, NOW).expect("add");

        let off = store.update_account(&a.id, None, Some(false)).expect("off");
        assert!(!off.enabled);
        assert_eq!(off.disabled_reason, Some(DisabledReason::User));

        let on = store.update_account(&a.id, None, Some(true)).expect("on");
        assert!(on.enabled);
        assert_eq!(on.disabled_reason, None);
    }

    #[test]
    fn renaming_an_account_keeps_everything_else() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let dir = make_dir(tmp.path(), ".claude3");
        let a = store.add_account(&dir, true, None, false, NOW).expect("add");

        let renamed = store
            .update_account(&a.id, Some("Work account"), None)
            .expect("rename");
        assert_eq!(renamed.label, "Work account");
        assert!(renamed.enabled);
        assert_eq!(renamed.config_dir, dir);
    }

    #[test]
    fn updating_an_unknown_account_is_not_found() {
        let store = Store::open_in_memory().expect("open");
        let err = store
            .update_account("nope", Some("x"), None)
            .expect_err("must reject");
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn mark_guard_tripped_disables_with_the_guard_reason() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let dir = make_dir(tmp.path(), ".claude3");
        let a = store.add_account(&dir, true, None, false, NOW).expect("add");

        store.mark_guard_tripped(&a.id).expect("mark");
        let back = store
            .account_by_id(&a.id)
            .expect("read")
            .expect("must exist");
        assert!(!back.enabled);
        assert_eq!(back.disabled_reason, Some(DisabledReason::GuardTripped));
    }

    #[test]
    fn enabled_account_ids_skips_disabled_rows_and_keeps_d17_order() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let def = make_dir(tmp.path(), ".claudeMain");
        let a = make_dir(tmp.path(), ".claudeA");
        let b = make_dir(tmp.path(), ".claudeB");
        let d = store.add_account(&def, true, None, true, NOW).expect("d");
        let ea = store.add_account(&a, true, None, false, NOW).expect("a");
        let eb = store.add_account(&b, true, None, false, NOW).expect("b");
        store
            .update_account(&eb.id, None, Some(false))
            .expect("disable b");

        assert_eq!(
            store.enabled_account_ids().expect("ids"),
            vec![d.id.clone(), ea.id.clone()]
        );
    }

    #[test]
    fn seeding_an_empty_store_enables_every_candidate_and_flags_the_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let one = make_dir(tmp.path(), ".claude");
        let two = make_dir(tmp.path(), ".claude3");

        let n = store
            .seed_accounts_if_empty(
                &[candidate(&one, "claude"), candidate(&two, "claude3")],
                &one,
                NOW,
            )
            .expect("seed");
        assert_eq!(n, 2);

        let all = store.list_accounts().expect("list");
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|a| a.enabled));
        assert!(all.iter().all(|a| a.disabled_reason.is_none()));
        assert_eq!(all[0].label, "claude");
        assert!(all[0].is_default);
        assert!(!all[1].is_default);
    }

    #[test]
    fn seeding_is_skipped_when_accounts_already_exist() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let one = make_dir(tmp.path(), ".claude");
        let two = make_dir(tmp.path(), ".claude3");
        store.add_account(&one, true, None, true, NOW).expect("add");

        let n = store
            .seed_accounts_if_empty(
                &[candidate(&one, "claude"), candidate(&two, "claude3")],
                &one,
                NOW,
            )
            .expect("seed");
        assert_eq!(n, 0);
        assert_eq!(store.list_accounts().expect("list").len(), 1);
    }

    #[test]
    fn rescan_adds_only_new_candidates_and_adds_them_disabled() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let one = make_dir(tmp.path(), ".claude");
        let two = make_dir(tmp.path(), ".claude3");
        store.add_account(&one, true, None, true, NOW).expect("add");

        let added = store
            .rescan_accounts(
                &[candidate(&one, "claude"), candidate(&two, "claude3")],
                NOW,
            )
            .expect("rescan");
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].label, "claude3");
        assert!(!added[0].enabled);
        assert_eq!(added[0].disabled_reason, Some(DisabledReason::User));
        assert_eq!(store.list_accounts().expect("list").len(), 2);

        let again = store
            .rescan_accounts(
                &[candidate(&one, "claude"), candidate(&two, "claude3")],
                NOW,
            )
            .expect("rescan again");
        assert!(again.is_empty());
    }

    #[test]
    fn removing_an_account_removes_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let dir = make_dir(tmp.path(), ".claude3");
        let a = store.add_account(&dir, true, None, false, NOW).expect("add");
        store.remove_account(&a.id).expect("remove");
        assert!(store.account_by_id(&a.id).expect("read").is_none());
    }

    #[test]
    fn removing_an_unknown_account_is_not_found() {
        let store = Store::open_in_memory().expect("open");
        let err = store.remove_account("nope").expect_err("must reject");
        assert_eq!(err.code(), "not_found");
    }
}
```

- [ ] **Step 2: Run test to verify it fails** — first add `pub mod accounts;` to `src-tauri/src/store/mod.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib store::accounts::
```

Expect `no method named 'add_account' found for struct 'Store'`.

- [ ] **Step 3: Write minimal implementation** — prepend to `src-tauri/src/store/accounts.rs`, above the test module:

```rust
use rusqlite::{params, Row};
use std::path::{Path, PathBuf};

use crate::discovery::Candidate;
use crate::error::{AppError, AppResult};
use crate::store::Store;
use crate::usage::{Account, DisabledReason};

/// D17: default account first, then case-insensitive label.
const ORDER_D17: &str = "ORDER BY is_default DESC, lower(label) ASC, label ASC";

fn row_to_account(row: &Row<'_>) -> Result<Account, rusqlite::Error> {
    let reason: Option<String> = row.get("disabled_reason")?;
    Ok(Account {
        id: row.get("id")?,
        label: row.get("label")?,
        config_dir: PathBuf::from(row.get::<_, String>("config_dir")?),
        enabled: row.get::<_, i64>("enabled")? != 0,
        disabled_reason: reason.as_deref().and_then(DisabledReason::from_wire),
        is_default: row.get::<_, i64>("is_default")? != 0,
        created_at: row.get("created_at")?,
    })
}

fn label_for(dir: &Path) -> String {
    dir.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.trim_start_matches('.').to_string())
        .unwrap_or_else(|| dir.to_string_lossy().to_string())
}

const SELECT_COLS: &str =
    "id, label, config_dir, enabled, disabled_reason, is_default, created_at";

impl Store {
    pub fn list_accounts(&self) -> AppResult<Vec<Account>> {
        self.with_conn(|c| {
            let sql = format!("SELECT {SELECT_COLS} FROM accounts {ORDER_D17}");
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt
                .query_map([], row_to_account)?
                .collect::<Result<Vec<Account>, rusqlite::Error>>()?;
            Ok(rows)
        })
    }

    pub fn enabled_account_ids(&self) -> AppResult<Vec<String>> {
        Ok(self
            .list_accounts()?
            .into_iter()
            .filter(|a| a.enabled)
            .map(|a| a.id)
            .collect())
    }

    pub fn account_by_id(&self, id: &str) -> AppResult<Option<Account>> {
        self.with_conn(|c| {
            let sql = format!("SELECT {SELECT_COLS} FROM accounts WHERE id = ?1");
            let mut stmt = c.prepare(&sql)?;
            let mut rows = stmt.query(params![id])?;
            match rows.next()? {
                Some(row) => Ok(Some(row_to_account(row)?)),
                None => Ok(None),
            }
        })
    }

    /// Canonicalises, rejects a missing directory (`not_found`) and a path
    /// already tracked (`duplicate`).
    pub fn add_account(
        &self,
        config_dir: &Path,
        enabled: bool,
        disabled_reason: Option<DisabledReason>,
        is_default: bool,
        now: i64,
    ) -> AppResult<Account> {
        if !config_dir.is_dir() {
            return Err(AppError::NotFound(format!(
                "no such directory: {}",
                config_dir.display()
            )));
        }
        let canonical = dunce::canonicalize(config_dir)
            .unwrap_or_else(|_| config_dir.to_path_buf());
        let canonical_str = canonical.to_string_lossy().to_string();

        let exists: i64 = self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM accounts WHERE config_dir = ?1",
                params![canonical_str],
                |r| r.get(0),
            )?)
        })?;
        if exists > 0 {
            return Err(AppError::Duplicate(format!(
                "already tracked: {canonical_str}"
            )));
        }

        let account = Account {
            id: uuid::Uuid::new_v4().to_string(),
            label: label_for(&canonical),
            config_dir: canonical,
            enabled,
            disabled_reason,
            is_default,
            created_at: now,
        };

        self.with_conn(|c| {
            c.execute(
                "INSERT INTO accounts(id, label, config_dir, enabled, disabled_reason, is_default, created_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    account.id,
                    account.label,
                    canonical_str,
                    i64::from(account.enabled),
                    account.disabled_reason.map(|r| r.as_str()),
                    i64::from(account.is_default),
                    account.created_at
                ],
            )?;
            Ok(())
        })?;

        Ok(account)
    }

    /// `enabled: false` sets `disabled_reason = user`; `enabled: true` clears it.
    pub fn update_account(
        &self,
        id: &str,
        label: Option<&str>,
        enabled: Option<bool>,
    ) -> AppResult<Account> {
        if self.account_by_id(id)?.is_none() {
            return Err(AppError::NotFound(format!("no such account: {id}")));
        }
        if let Some(l) = label {
            self.with_conn(|c| {
                c.execute(
                    "UPDATE accounts SET label = ?2 WHERE id = ?1",
                    params![id, l],
                )?;
                Ok(())
            })?;
        }
        if let Some(on) = enabled {
            let reason = if on { None } else { Some(DisabledReason::User.as_str()) };
            self.with_conn(|c| {
                c.execute(
                    "UPDATE accounts SET enabled = ?2, disabled_reason = ?3 WHERE id = ?1",
                    params![id, i64::from(on), reason],
                )?;
                Ok(())
            })?;
        }
        self.account_by_id(id)?
            .ok_or_else(|| AppError::NotFound(format!("no such account: {id}")))
    }

    /// Spec 6.3: the tripped account is disabled with the guard reason so the
    /// row explains which account tripped.
    pub fn mark_guard_tripped(&self, id: &str) -> AppResult<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE accounts SET enabled = 0, disabled_reason = ?2 WHERE id = ?1",
                params![id, DisabledReason::GuardTripped.as_str()],
            )?;
            Ok(())
        })
    }

    pub fn remove_account(&self, id: &str) -> AppResult<()> {
        let removed = self.with_conn(|c| {
            Ok(c.execute("DELETE FROM accounts WHERE id = ?1", params![id])?)
        })?;
        if removed == 0 {
            return Err(AppError::NotFound(format!("no such account: {id}")));
        }
        Ok(())
    }

    /// Spec 6.1: first start seeds every candidate **enabled**.
    pub fn seed_accounts_if_empty(
        &self,
        candidates: &[Candidate],
        default_dir: &Path,
        now: i64,
    ) -> AppResult<usize> {
        let existing: i64 =
            self.with_conn(|c| Ok(c.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?))?;
        if existing > 0 {
            return Ok(0);
        }
        let default_canonical =
            dunce::canonicalize(default_dir).unwrap_or_else(|_| default_dir.to_path_buf());

        let mut added = 0usize;
        for c in candidates {
            let is_default = c.config_dir == default_canonical;
            match self.add_account(&c.config_dir, true, None, is_default, now) {
                Ok(_) => added += 1,
                Err(AppError::Duplicate(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(added)
    }

    /// Spec 6.1: a rescan adds new candidates **disabled** (reason `user`).
    pub fn rescan_accounts(
        &self,
        candidates: &[Candidate],
        now: i64,
    ) -> AppResult<Vec<Account>> {
        let known: Vec<PathBuf> = self
            .list_accounts()?
            .into_iter()
            .map(|a| a.config_dir)
            .collect();

        let mut added = Vec::new();
        for c in candidates {
            if known.iter().any(|k| k == &c.config_dir) {
                continue;
            }
            match self.add_account(
                &c.config_dir,
                false,
                Some(DisabledReason::User),
                false,
                now,
            ) {
                Ok(a) => added.push(a),
                Err(AppError::Duplicate(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(added)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib store::accounts::
cargo clippy --all-targets -- -D warnings
```

Expect 14 passing tests, no warnings.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 10: add account storage with D17 ordering, seeding and rescan

Adds account CRUD keyed on the canonical config dir with duplicate and
not-found rejection, the default-first case-insensitive ordering, the
enable/disable reason rules including the guard-tripped marker, first-start
seeding that enables every candidate, and rescan which adds new candidates
disabled.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 11: Store — snapshots, history and pruning

**Files:** Create `src-tauri/src/store/snapshots.rs`; Modify `src-tauri/src/store/mod.rs`
**Interfaces:** Consumes: `Store` (Task 8), `PollOutcome` / `SnapshotDto` / `ModelWindow` / `Window` / `OutcomeKind` (Task 3). Produces: `pub struct HistoryPoint { t: i64, pct: u8 }`, `pub const RETENTION_MS: i64`, `Store::insert_snapshot(&str, i64, &PollOutcome, Option<&str>, u32) -> AppResult<i64>`, `Store::latest_per_account() -> AppResult<HashMap<String, SnapshotDto>>`, `Store::history(&str, i64) -> AppResult<Vec<HistoryPoint>>`, `Store::prune(i64) -> AppResult<usize>`, `Store::snapshot_raw(i64) -> AppResult<(Option<String>, Option<String>)>`.

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/store/snapshots.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::usage::{Parsed, Window};

    const NOW: i64 = 1_700_000_000_000;
    const HOUR: i64 = 3_600_000;

    fn store_with_account() -> (tempfile::TempDir, Store, String) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let dir = tmp.path().join(".claude3");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let a = store
            .add_account(&dir, true, None, true, NOW)
            .expect("add account");
        (tmp, store, a.id)
    }

    fn ok_outcome(session: u8, week: u8) -> PollOutcome {
        PollOutcome::Ok(Parsed {
            session: Window { pct: session, resets_at: Some(NOW + HOUR) },
            week_all: Window { pct: week, resets_at: Some(NOW + 6 * HOUR) },
            week_models: vec![(
                "Fable".to_string(),
                Window { pct: 5, resets_at: Some(NOW + 6 * HOUR) },
            )],
        })
    }

    #[test]
    fn an_ok_snapshot_round_trips_into_a_dto() {
        let (_tmp, store, acct) = store_with_account();
        let id = store
            .insert_snapshot(&acct, NOW, &ok_outcome(15, 4), Some("raw text"), 3012)
            .expect("insert");
        assert!(id > 0);

        let latest = store.latest_per_account().expect("latest");
        let dto = latest.get(&acct).expect("row for account");
        assert_eq!(dto.outcome, "ok");
        assert_eq!(dto.session, Some(Window { pct: 15, resets_at: Some(NOW + HOUR) }));
        assert_eq!(
            dto.week_all,
            Some(Window { pct: 4, resets_at: Some(NOW + 6 * HOUR) })
        );
        assert_eq!(dto.week_models.len(), 1);
        assert_eq!(dto.week_models[0].label, "Fable");
        assert_eq!(dto.week_models[0].pct, 5);
        assert_eq!(dto.error, None);
        assert_eq!(dto.duration_ms, 3012);
    }

    #[test]
    fn a_failure_snapshot_stores_its_message_and_no_windows() {
        let (_tmp, store, acct) = store_with_account();
        store
            .insert_snapshot(
                &acct,
                NOW,
                &PollOutcome::SpawnError("exit 1: boom".into()),
                Some("stderr tail"),
                412,
            )
            .expect("insert");

        let latest = store.latest_per_account().expect("latest");
        let dto = latest.get(&acct).expect("row");
        assert_eq!(dto.outcome, "spawn_error");
        assert_eq!(dto.session, None);
        assert_eq!(dto.week_all, None);
        assert!(dto.week_models.is_empty());
        assert_eq!(dto.error.as_deref(), Some("exit 1: boom"));
    }

    #[test]
    fn raw_is_stored_for_every_outcome() {
        let (_tmp, store, acct) = store_with_account();
        let ok_id = store
            .insert_snapshot(&acct, NOW, &ok_outcome(1, 1), Some("ok raw"), 10)
            .expect("insert ok");
        let fail_id = store
            .insert_snapshot(
                &acct,
                NOW + 1,
                &PollOutcome::Timeout(30),
                Some("timeout raw"),
                30_000,
            )
            .expect("insert fail");

        assert_eq!(
            store.snapshot_raw(ok_id).expect("raw"),
            (Some("ok raw".to_string()), None)
        );
        assert_eq!(
            store.snapshot_raw(fail_id).expect("raw"),
            (
                Some("timeout raw".to_string()),
                Some("timed out after 30s".to_string())
            )
        );
    }

    #[test]
    fn snapshot_raw_returns_the_error_text_too() {
        let (_tmp, store, acct) = store_with_account();
        let id = store
            .insert_snapshot(
                &acct,
                NOW,
                &PollOutcome::ParseError("missing session line".into()),
                Some("report text"),
                900,
            )
            .expect("insert");
        assert_eq!(
            store.snapshot_raw(id).expect("raw"),
            (
                Some("report text".to_string()),
                Some("missing session line".to_string())
            )
        );
    }

    #[test]
    fn snapshot_raw_for_an_unknown_id_is_not_found() {
        let (_tmp, store, _acct) = store_with_account();
        let err = store.snapshot_raw(9999).expect_err("must reject");
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn latest_per_account_breaks_a_same_millisecond_tie_on_the_highest_id() {
        let (_tmp, store, acct) = store_with_account();
        store
            .insert_snapshot(&acct, NOW, &ok_outcome(10, 10), None, 1)
            .expect("first");
        store
            .insert_snapshot(&acct, NOW, &ok_outcome(20, 20), None, 1)
            .expect("second");

        let latest = store.latest_per_account().expect("latest");
        let dto = latest.get(&acct).expect("row");
        assert_eq!(dto.session.map(|w| w.pct), Some(20));
    }

    #[test]
    fn history_buckets_by_hour_and_takes_the_maximum_week_all_pct() {
        let (_tmp, store, acct) = store_with_account();
        let h0 = 1_700_000_000_000 - (1_700_000_000_000 % HOUR);
        store
            .insert_snapshot(&acct, h0 + 60_000, &ok_outcome(1, 4), None, 1)
            .expect("a");
        store
            .insert_snapshot(&acct, h0 + 120_000, &ok_outcome(1, 9), None, 1)
            .expect("b");
        store
            .insert_snapshot(&acct, h0 + HOUR + 60_000, &ok_outcome(1, 6), None, 1)
            .expect("c");

        let points = store.history(&acct, h0 - HOUR).expect("history");
        assert_eq!(points.len(), 2);
        assert_eq!(points[0], HistoryPoint { t: h0, pct: 9 });
        assert_eq!(points[1], HistoryPoint { t: h0 + HOUR, pct: 6 });
    }

    #[test]
    fn history_leaves_empty_hours_out_entirely() {
        let (_tmp, store, acct) = store_with_account();
        let h0 = 1_700_000_000_000 - (1_700_000_000_000 % HOUR);
        store
            .insert_snapshot(&acct, h0 + 1000, &ok_outcome(1, 4), None, 1)
            .expect("a");
        // Skip three hours entirely, as the process gate does overnight.
        store
            .insert_snapshot(&acct, h0 + 4 * HOUR + 1000, &ok_outcome(1, 7), None, 1)
            .expect("b");

        let points = store.history(&acct, h0 - HOUR).expect("history");
        assert_eq!(points.len(), 2, "no zero-filling of the gap");
        assert_eq!(points[0].t, h0);
        assert_eq!(points[1].t, h0 + 4 * HOUR);
    }

    #[test]
    fn history_ignores_non_ok_rows() {
        let (_tmp, store, acct) = store_with_account();
        let h0 = 1_700_000_000_000 - (1_700_000_000_000 % HOUR);
        store
            .insert_snapshot(&acct, h0 + 1000, &PollOutcome::Timeout(30), None, 1)
            .expect("a");
        store
            .insert_snapshot(&acct, h0 + 2000, &PollOutcome::NoUsageData, None, 1)
            .expect("b");
        assert!(store.history(&acct, h0 - HOUR).expect("history").is_empty());
    }

    #[test]
    fn prune_deletes_rows_strictly_older_than_the_retention_window() {
        let (_tmp, store, acct) = store_with_account();
        let boundary = NOW - RETENTION_MS;
        store
            .insert_snapshot(&acct, boundary - 1, &ok_outcome(1, 1), None, 1)
            .expect("older");
        store
            .insert_snapshot(&acct, boundary, &ok_outcome(2, 2), None, 1)
            .expect("exactly at the boundary");
        store
            .insert_snapshot(&acct, boundary + 1, &ok_outcome(3, 3), None, 1)
            .expect("newer");

        let removed = store.prune(NOW).expect("prune");
        assert_eq!(removed, 1);

        let remaining: i64 = store
            .with_conn(|c| Ok(c.query_row("SELECT COUNT(*) FROM snapshots", [], |r| r.get(0))?))
            .expect("count");
        assert_eq!(remaining, 2);
    }

    #[test]
    fn prune_on_an_empty_table_removes_nothing() {
        let (_tmp, store, _acct) = store_with_account();
        assert_eq!(store.prune(NOW).expect("prune"), 0);
    }

    #[test]
    fn removing_an_account_cascades_its_snapshots() {
        let (_tmp, store, acct) = store_with_account();
        store
            .insert_snapshot(&acct, NOW, &ok_outcome(1, 1), Some("raw"), 1)
            .expect("insert");
        store.remove_account(&acct).expect("remove");

        let remaining: i64 = store
            .with_conn(|c| Ok(c.query_row("SELECT COUNT(*) FROM snapshots", [], |r| r.get(0))?))
            .expect("count");
        assert_eq!(remaining, 0, "foreign_keys=ON must cascade the delete");
    }

    #[test]
    fn latest_per_account_covers_several_accounts_independently() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let mut ids = Vec::new();
        for name in [".claude", ".claude3"] {
            let d = tmp.path().join(name);
            std::fs::create_dir_all(&d).expect("mkdir");
            ids.push(store.add_account(&d, true, None, false, NOW).expect("add").id);
        }
        store
            .insert_snapshot(&ids[0], NOW, &ok_outcome(11, 11), None, 1)
            .expect("a");
        store
            .insert_snapshot(&ids[1], NOW, &ok_outcome(22, 22), None, 1)
            .expect("b");

        let latest = store.latest_per_account().expect("latest");
        assert_eq!(latest.len(), 2);
        assert_eq!(latest[&ids[0]].session.map(|w| w.pct), Some(11));
        assert_eq!(latest[&ids[1]].session.map(|w| w.pct), Some(22));
    }
}
```

- [ ] **Step 2: Run test to verify it fails** — first add `pub mod snapshots;` and `pub use snapshots::HistoryPoint;` to `src-tauri/src/store/mod.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib store::snapshots::
```

Expect `no method named 'insert_snapshot' found for struct 'Store'` and `cannot find type 'HistoryPoint' in this scope`.

- [ ] **Step 3: Write minimal implementation** — prepend to `src-tauri/src/store/snapshots.rs`, above the test module:

```rust
use rusqlite::{params, Row};
use serde::Serialize;
use std::collections::HashMap;
use tracing::warn;

use crate::error::{AppError, AppResult};
use crate::store::Store;
use crate::usage::{ModelWindow, OutcomeKind, PollOutcome, SnapshotDto, Window};

/// D10: 30 days, in milliseconds.
pub const RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;

const HOUR_MS: i64 = 3_600_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HistoryPoint {
    pub t: i64,
    pub pct: u8,
}

const DTO_COLS: &str = "id, account_id, taken_at, outcome, session_pct, session_resets_at, \
                        week_all_pct, week_all_resets_at, week_models, error, duration_ms";

fn window_from_cols(pct: Option<i64>, resets_at: Option<i64>) -> Option<Window> {
    pct.map(|p| Window {
        pct: p.clamp(0, 100) as u8,
        resets_at,
    })
}

fn row_to_dto(row: &Row<'_>) -> Result<SnapshotDto, rusqlite::Error> {
    let outcome_raw: String = row.get("outcome")?;
    let outcome = OutcomeKind::from_wire(&outcome_raw)
        .unwrap_or(OutcomeKind::ParseError)
        .as_str();
    let models_json: Option<String> = row.get("week_models")?;
    let week_models: Vec<ModelWindow> = models_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<Vec<ModelWindow>>(j).ok())
        .unwrap_or_default();

    Ok(SnapshotDto {
        id: row.get("id")?,
        account_id: row.get("account_id")?,
        taken_at: row.get("taken_at")?,
        outcome,
        session: window_from_cols(row.get("session_pct")?, row.get("session_resets_at")?),
        week_all: window_from_cols(row.get("week_all_pct")?, row.get("week_all_resets_at")?),
        week_models,
        error: row.get("error")?,
        duration_ms: row.get::<_, i64>("duration_ms")?.clamp(0, i64::from(u32::MAX)) as u32,
    })
}

impl Store {
    /// D8: raw text is stored with every snapshot, success or failure.
    pub fn insert_snapshot(
        &self,
        account_id: &str,
        taken_at: i64,
        outcome: &PollOutcome,
        raw: Option<&str>,
        duration_ms: u32,
    ) -> AppResult<i64> {
        let (session, week_all, models_json) = match outcome {
            PollOutcome::Ok(p) => {
                let models: Vec<ModelWindow> = p
                    .week_models
                    .iter()
                    .map(|(label, w)| ModelWindow {
                        label: label.clone(),
                        pct: w.pct,
                        resets_at: w.resets_at,
                    })
                    .collect();
                (
                    Some(p.session),
                    Some(p.week_all),
                    Some(serde_json::to_string(&models)?),
                )
            }
            _ => (None, None, None),
        };

        self.with_conn(|c| {
            c.execute(
                "INSERT INTO snapshots(account_id, taken_at, outcome, session_pct,
                     session_resets_at, week_all_pct, week_all_resets_at, week_models,
                     error, raw, duration_ms)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    account_id,
                    taken_at,
                    outcome.kind().as_str(),
                    session.map(|w| i64::from(w.pct)),
                    session.and_then(|w| w.resets_at),
                    week_all.map(|w| i64::from(w.pct)),
                    week_all.and_then(|w| w.resets_at),
                    models_json,
                    outcome.error_text(),
                    raw,
                    i64::from(duration_ms),
                ],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    /// Newest snapshot per account: max `taken_at`, tiebreak max `id`.
    pub fn latest_per_account(&self) -> AppResult<HashMap<String, SnapshotDto>> {
        self.with_conn(|c| {
            let sql = format!(
                "SELECT {DTO_COLS} FROM snapshots s
                 WHERE s.id = (SELECT x.id FROM snapshots x
                               WHERE x.account_id = s.account_id
                               ORDER BY x.taken_at DESC, x.id DESC LIMIT 1)"
            );
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt
                .query_map([], row_to_dto)?
                .collect::<Result<Vec<SnapshotDto>, rusqlite::Error>>()?;
            Ok(rows
                .into_iter()
                .map(|d| (d.account_id.clone(), d))
                .collect())
        })
    }

    /// Hourly buckets of `max(week_all_pct)` over `ok` rows, only for hours
    /// that have at least one row. No zero-filling: the process gate
    /// guarantees overnight gaps and those must render as breaks.
    pub fn history(&self, account_id: &str, since: i64) -> AppResult<Vec<HistoryPoint>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT (taken_at / ?3) * ?3 AS bucket, MAX(week_all_pct) AS pct
                 FROM snapshots
                 WHERE account_id = ?1 AND taken_at >= ?2
                   AND outcome = 'ok' AND week_all_pct IS NOT NULL
                 GROUP BY bucket
                 ORDER BY bucket ASC",
            )?;
            let rows = stmt
                .query_map(params![account_id, since, HOUR_MS], |r| {
                    Ok(HistoryPoint {
                        t: r.get::<_, i64>("bucket")?,
                        pct: r.get::<_, i64>("pct")?.clamp(0, 100) as u8,
                    })
                })?
                .collect::<Result<Vec<HistoryPoint>, rusqlite::Error>>()?;
            Ok(rows)
        })
    }

    /// D10: drop rows older than the retention window, then reclaim pages.
    pub fn prune(&self, now: i64) -> AppResult<usize> {
        let cutoff = now - RETENTION_MS;
        let removed = self.with_conn(|c| {
            Ok(c.execute(
                "DELETE FROM snapshots WHERE taken_at < ?1",
                params![cutoff],
            )?)
        })?;
        if removed > 0 {
            warn!(removed, cutoff, "pruned snapshots older than the retention window");
        }
        self.with_conn(|c| {
            c.execute_batch("PRAGMA incremental_vacuum;")?;
            Ok(())
        })?;
        Ok(removed)
    }

    /// Backing store for `get_snapshot_raw`: `(raw, error)`.
    pub fn snapshot_raw(&self, snapshot_id: i64) -> AppResult<(Option<String>, Option<String>)> {
        self.with_conn(|c| {
            let mut stmt =
                c.prepare("SELECT raw, error FROM snapshots WHERE id = ?1")?;
            let mut rows = stmt.query(params![snapshot_id])?;
            match rows.next()? {
                Some(row) => Ok((row.get(0)?, row.get(1)?)),
                None => Err(AppError::NotFound(format!(
                    "no such snapshot: {snapshot_id}"
                ))),
            }
        })
    }
}
```

- [ ] **Step 4: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib store::snapshots::
cargo clippy --all-targets -- -D warnings
```

Expect 13 passing tests, no warnings.

- [ ] **Step 5: Run the whole suite** — the store now spans four modules; confirm nothing regressed:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test
```

- [ ] **Step 6: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 11: add snapshot persistence, hourly history and retention pruning

Persists raw text with every outcome, rebuilds SnapshotDto from a row,
resolves the newest snapshot per account with a same-millisecond id
tiebreak, buckets week-all history by hour over ok rows only with no
zero-filling, and prunes past the 30 day window followed by an incremental
vacuum.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 12: Scheduler state machine

**Files:** Create `src-tauri/src/scheduler/mod.rs`, `src-tauri/src/scheduler/machine.rs`; Modify `src-tauri/src/lib.rs`
**Interfaces:** Consumes: `OutcomeKind`, `PollOutcome`, `is_unexpected_envelope`, `UNEXPECTED_ENVELOPE_PREFIX` (Task 3). Produces: `pub enum Gate`, `pub enum Trigger`, `pub enum SkipReason`, `pub enum Decision`, `pub enum Recorded { Continue, Escalate }`, `pub struct Backoff { consecutive_failures: u32, next_allowed: i64, unexpected_envelope_streak: u8 }`, `pub struct DriverStatus { gate: Gate, busy: bool, stalled_at: Option<i64>, backoff_until: HashMap<String, i64> }`, `pub const MAX_ENVELOPE_STRIKES: u8 = 5`, `pub struct Machine`, `pub struct CycleToken`, `pub type SharedMachine = std::sync::Arc<std::sync::Mutex<Machine>>`, `pub fn lock_machine(&Mutex<Machine>) -> MutexGuard<'_, Machine>`, `pub fn begin_cycle(&SharedMachine, i64) -> CycleToken`, `pub fn preview_manual(&DriverStatus, bool, bool, &[String]) -> Option<SkipReason>`, `Machine::decide(&mut self, Trigger, Option<bool>, bool, bool, &[String], i64) -> Decision`, `Machine::is_busy()`, `Machine::gate()`, `Machine::status(&self, i64) -> DriverStatus`, `Machine::record(&str, &PollOutcome, i64) -> Recorded`, `Machine::reset_backoff(&str)`, `Machine::reset_all_backoff()`, `Machine::backoff_until(&str) -> Option<i64>`, `Machine::cycle_age(i64) -> Option<std::time::Duration>`.

`Machine` is owned by the driver alone. Nothing outside `driver.rs` ever reads its fields: the driver publishes a `DriverStatus` snapshot into shared state after every `decide()` and every `record()`, and that snapshot is the only scheduler state commands and the UI ever see (spec §5.1). `DriverStatus` is defined here because it is built from `Gate` and the backoff map.

Three spec points are resolved here and each resolution is written into the code as a comment:
1. `AccountChanged(ids)` whose intersection with the enabled set is empty skips with `NoEnabledAccounts`, not `AllBackedOff` — nothing was in cooldown, the changed accounts simply are not enabled. `AllBackedOff` keeps its spec meaning of "enabled accounts exist and every one is in cooldown".
2. The RAII `CycleToken` needs shared ownership to clear busy on drop, so the driver holds the machine as `Arc<Mutex<Machine>>`. `decide` itself stays a pure function of its arguments and the machine's fields.
3. `Machine::status` cannot know `stalled_at` — the watchdog owns that — so it returns `None` there and the driver carries the previous value across when it publishes. `backoff_until` lists every account with a live failure streak rather than filtering on `now`, because the snapshot is written when the driver acts and read later by the UI, which already ignores an elapsed deadline.

- [ ] **Step 1: Write the failing test for the decision table** — create `src-tauri/src/scheduler/machine.rs` containing only this test module for now:

```rust
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

    #[test]
    fn reset_backoff_also_clears_the_envelope_streak() {
        let mut m = Machine::new();
        for _ in 0..4 {
            assert_eq!(m.record("a", &shape_failure(), NOW), Recorded::Continue);
        }
        m.reset_backoff("a");
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
```

- [ ] **Step 2: Run test to verify it fails** — first create `src-tauri/src/scheduler/mod.rs` with `pub mod machine;`, add `pub mod scheduler;` to `src-tauri/src/lib.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib scheduler::machine::
```

Expect `cannot find type 'Machine' in this scope` and `cannot find function 'begin_cycle' in this scope`.

- [ ] **Step 3: Write minimal implementation** — prepend to `src-tauri/src/scheduler/machine.rs`, above the test module:

```rust
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

    pub fn reset_backoff(&mut self, account: &str) {
        self.backoff.remove(account);
    }

    /// Used by the settings watch (D16).
    pub fn reset_all_backoff(&mut self) {
        self.backoff.clear();
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
```

- [ ] **Step 4: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib scheduler::machine::
cargo clippy --all-targets -- -D warnings
```

Expect 35 passing tests, which is every `#[test]` in `scheduler::machine`, and no warnings. The panic test prints an unwind message; that is expected output, not a failure.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 12: add the pure scheduler state machine

Implements decide in spec order (halt, busy, no binary, no enabled
accounts, candidate list, backoff filter, gate transition) returning a
description the driver executes, plus the D16 doubling backoff capped at
15 minutes and an RAII cycle token that clears busy even when the cycle
task panics. record now carries the unexpected-envelope streak and returns
Escalate on the fifth consecutive unclassifiable envelope, and status
produces the DriverStatus snapshot that is the only scheduler state
anything outside the driver ever reads. A skipped decision never moves the
gate, so the promised final poll survives backoff.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 13: `fake_claude` test helper binary

**Files:** Modify `src-tauri/src/bin/fake_claude.rs`; Create `src-tauri/tests/fake_claude_modes.rs`
**Interfaces:** Consumes: nothing (a standalone binary, no dependency on `cut_core`). Produces: a binary reachable from integration tests as `env!("CARGO_BIN_EXE_fake_claude")`, driven entirely by environment variables: `FAKE_CLAUDE_MODE` ∈ `emit` | `slow` | `echo-env` | `echo-argv` | `exit-nonzero` | `non-json`, plus `FAKE_CLAUDE_STDOUT`, `FAKE_CLAUDE_STDERR`, `FAKE_CLAUDE_SLEEP_SECS`, `FAKE_CLAUDE_EXIT`.

It is excluded from the shipped bundle by `mainBinaryName` in `tauri.conf.json` (Task 1), which is what the Tauri bundler ships.

- [ ] **Step 1: Write the failing test** — create `src-tauri/tests/fake_claude_modes.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --test fake_claude_modes
```

Expect every test to fail: the placeholder binary from Task 1 prints nothing, so the stdout assertions fail and the JSON parses fail.

- [ ] **Step 3: Write minimal implementation** — replace `src-tauri/src/bin/fake_claude.rs` with:

```rust
//! Test-only stand-in for the Claude Code binary. Behaviour is chosen
//! entirely by environment variables so integration tests can drive it
//! through the same spawn path the real runner uses. Excluded from the
//! shipped bundle by `mainBinaryName` in tauri.conf.json.

use std::io::Write;

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
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
            let mut lines: Vec<String> = std::env::vars()
                .filter(|(k, _)| k.starts_with("ANTHROPIC_") || k.starts_with("CLAUDE_"))
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
            std::thread::sleep(std::time::Duration::from_secs(secs));
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
```

- [ ] **Step 4: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --test fake_claude_modes
cargo clippy --all-targets -- -D warnings
```

Expect 6 passing tests, no warnings. `unwrap_or` and `unwrap_or_else` are used here rather than `unwrap`, so the no-panic rule holds even in the helper.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 13: add the fake_claude test helper binary

Provides emit, echo-argv, echo-env, exit-nonzero, non-json and slow modes
selected by environment variables, so runner integration tests can exercise
the real spawn path without touching the actual CLI or spending quota. The
bundle ships only the main binary, so this never reaches a release.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 14: Envelope guard (pure)

**Files:** Create `src-tauri/src/usage/runner.rs`; Modify `src-tauri/src/usage/mod.rs`
**Interfaces:** Consumes: `UNEXPECTED_ENVELOPE_PREFIX` (Task 3) and `serde_json`. Produces: `pub const USAGE_ARGV: [&str; 11]`, `pub enum GuardVerdict { Usage(String), Shape(String), Tripped(String) }`, `pub fn check_envelope(stdout: &str) -> GuardVerdict`, `pub fn env_names_to_strip(names: impl Iterator<Item = String>) -> Vec<String>`.

Spec 6.3 step 5 (the five-strike escalation) is **not** here. v5 puts the counter in `Backoff.unexpected_envelope_streak` and the decision in `Machine::record`, both written in Task 12, so the guard stays a pure classifier of one envelope.

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/usage/runner.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

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
    fn only_anthropic_and_claude_prefixed_names_are_stripped() {
        let names = [
            "ANTHROPIC_API_KEY",
            "CLAUDE_CONFIG_DIR",
            "CLAUDE_CODE_USE_BEDROCK",
            "PATH",
            "HOME",
            "MY_CLAUDE_THING",
            "anthropic_lowercase",
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
            ]
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails** — first add `pub mod runner;` to `src-tauri/src/usage/mod.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib usage::runner::
```

Expect `cannot find value 'USAGE_ARGV' in this scope` and `cannot find function 'check_envelope' in this scope`.

- [ ] **Step 3: Write minimal implementation** — prepend to `src-tauri/src/usage/runner.rs`, above the test module:

```rust
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
pub fn env_names_to_strip(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = names
        .filter(|n| n.starts_with("ANTHROPIC_") || n.starts_with("CLAUDE_"))
        .collect();
    out.sort();
    out.dedup();
    out
}
```

- [ ] **Step 4: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib usage::runner::
cargo clippy --all-targets -- -D warnings
```

Expect 19 passing tests, no warnings.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 14: add the envelope guard and the env strip list

Implements the spec 6.3 guard in order: shape, then local_command as the
primary turn evidence checked before any further shape test, then the
advisory num_turns / total_cost_usd / modelUsage rules, then the remaining
shape checks. Every shape verdict carries the shared prefix so the state
machine can count it towards the five-strike escalation, which lives in
Machine::record rather than here. Also adds the single argv constant and
the D15 list of environment names to remove.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 15: `run_usage` — spawn, sanitise, time out

**Files:** Modify `src-tauri/src/usage/runner.rs`; Create `src-tauri/tests/runner_guard.rs`
**Interfaces:** Consumes: `USAGE_ARGV`, `check_envelope`, `env_names_to_strip` (Task 14), `parse_usage` (Task 4), `PollOutcome` and `is_unexpected_envelope` (Task 3). Produces: `pub struct RunResult { outcome: PollOutcome, raw: Option<String>, duration_ms: u32 }`, `pub async fn run_usage(binary: &Path, config_dir: &Path, cwd: &Path, timeout: std::time::Duration, now: chrono::DateTime<chrono::Utc>, pid_slot: &std::sync::atomic::AtomicU32, cancel: &tokio_util::sync::CancellationToken, log_env_at_info: bool) -> RunResult`.

Spec resolution recorded in the code: spec 6.5 describes the live `Child` as sitting behind a `Mutex<Option<Child>>` shared with the driver. Holding a child handle in a shared async mutex while awaiting its exit would deadlock the shutdown path that wants to lock the same mutex to kill it, so the cycle task owns the `Child` outright and shutdown reaches it through the `CancellationToken` instead. The child is still killed only through its handle, never by raw PID; the PID is published solely as `exclude_pid` for the process check.

- [ ] **Step 1: Write the failing test for argv and environment** — create `src-tauri/tests/runner_guard.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --test runner_guard
```

Expect `cannot find function 'run_usage' in 'cut_core::usage::runner'` and `cannot find type 'RunResult'`.

- [ ] **Step 3: Write minimal implementation** — insert this block into `src-tauri/src/usage/runner.rs` between the existing implementation and the test module:

```rust
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
        let mut out = String::new();
        let mut err = String::new();
        if let Some(p) = stdout_pipe.as_mut() {
            let _ = p.read_to_string(&mut out).await;
        }
        if let Some(p) = stderr_pipe.as_mut() {
            let _ = p.read_to_string(&mut err).await;
        }
        (out, err)
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

    pid_slot.store(0, Ordering::SeqCst);
    let (stdout, stderr) = reader.await.unwrap_or_else(|_| (String::new(), String::new()));

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
            Some(stdout),
            started,
        );
    }

    let raw = Some(stdout.clone());
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
```

- [ ] **Step 4: Run the integration tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --test runner_guard -- --test-threads=1
```

The single test thread matters: these tests set process-wide environment variables to configure the fake binary. Expect 13 passing tests.

- [ ] **Step 5: Run the full suite and the linter** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 6: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 15: add run_usage with direct spawn, env sanitisation and timeout

Spawns the CLI directly with the single argv constant, strips every
ANTHROPIC_ and CLAUDE_ variable before setting CLAUDE_CONFIG_DIR, drains
both pipes concurrently, kills and waits on timeout or cancellation, and
routes the envelope through the guard so a turn envelope becomes
GuardTripped while an unclassifiable one becomes a retryable spawn error.
Raw stdout is kept for every outcome.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 16: Tray state (pure)

**Files:** Create `src-tauri/src/tray.rs`; Modify `src-tauri/src/lib.rs`
**Interfaces:** Consumes: `Account`, `SnapshotDto` (Task 3). Produces: `pub enum Level { Halted, Grey, Green, Amber, Red }` with `as_str()`, `pub fn tray_state(latest: &[(Account, Option<SnapshotDto>)], halted: bool) -> (Level, String)`.

`apply_tray`, which pushes the level and tooltip onto the real tray icon, is added in Task 21 where the Tauri handle exists.

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/tray.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{ModelWindow, SnapshotDto, Window};

    fn account(label: &str, enabled: bool) -> Account {
        Account {
            id: format!("id-{label}"),
            label: label.to_string(),
            config_dir: std::path::PathBuf::from(format!("/home/josh/.{label}")),
            enabled,
            disabled_reason: None,
            is_default: false,
            created_at: 0,
        }
    }

    fn ok_snapshot(session: u8, week: u8, models: &[(&str, u8)]) -> SnapshotDto {
        SnapshotDto {
            id: 1,
            account_id: "x".into(),
            taken_at: 0,
            outcome: "ok",
            session: Some(Window { pct: session, resets_at: None }),
            week_all: Some(Window { pct: week, resets_at: None }),
            week_models: models
                .iter()
                .map(|(l, p)| ModelWindow {
                    label: (*l).to_string(),
                    pct: *p,
                    resets_at: None,
                })
                .collect(),
            error: None,
            duration_ms: 1,
        }
    }

    fn failed_snapshot(outcome: &'static str) -> SnapshotDto {
        SnapshotDto {
            id: 2,
            account_id: "x".into(),
            taken_at: 0,
            outcome,
            session: None,
            week_all: None,
            week_models: vec![],
            error: Some("boom".into()),
            duration_ms: 1,
        }
    }

    #[test]
    fn level_wire_forms_are_snake_case() {
        assert_eq!(Level::Halted.as_str(), "halted");
        assert_eq!(Level::Grey.as_str(), "grey");
        assert_eq!(Level::Green.as_str(), "green");
        assert_eq!(Level::Amber.as_str(), "amber");
        assert_eq!(Level::Red.as_str(), "red");
    }

    #[test]
    fn halted_beats_everything_including_a_healthy_account() {
        let rows = vec![(account("claude", true), Some(ok_snapshot(1, 1, &[])))];
        let (level, tooltip) = tray_state(&rows, true);
        assert_eq!(level, Level::Halted);
        assert_eq!(tooltip, "polling halted — guard tripped");
    }

    #[test]
    fn no_ok_snapshot_anywhere_is_grey() {
        let rows = vec![
            (account("claude", true), None),
            (account("claude3", true), Some(failed_snapshot("timeout"))),
        ];
        let (level, _) = tray_state(&rows, false);
        assert_eq!(level, Level::Grey);
    }

    #[test]
    fn no_enabled_accounts_at_all_is_grey() {
        let rows = vec![(account("claude", false), Some(ok_snapshot(99, 99, &[])))];
        let (level, _) = tray_state(&rows, false);
        assert_eq!(level, Level::Grey);
    }

    #[test]
    fn thresholds_are_green_below_seventy_amber_to_eighty_nine_red_from_ninety() {
        for (pct, want) in [
            (0u8, Level::Green),
            (69, Level::Green),
            (70, Level::Amber),
            (89, Level::Amber),
            (90, Level::Red),
            (100, Level::Red),
        ] {
            let rows = vec![(account("claude", true), Some(ok_snapshot(pct, 0, &[])))];
            let (level, _) = tray_state(&rows, false);
            assert_eq!(level, want, "pct {pct}");
        }
    }

    #[test]
    fn the_worst_window_across_every_account_wins() {
        let rows = vec![
            (account("claude", true), Some(ok_snapshot(10, 20, &[("Fable", 30)]))),
            (account("claude3", true), Some(ok_snapshot(5, 5, &[("Opus", 95)]))),
        ];
        let (level, _) = tray_state(&rows, false);
        assert_eq!(level, Level::Red, "a per-model line counts too");
    }

    #[test]
    fn a_disabled_account_is_ignored_for_both_level_and_tooltip() {
        let rows = vec![
            (account("claude", true), Some(ok_snapshot(10, 10, &[]))),
            (account("claude-old", false), Some(ok_snapshot(99, 99, &[]))),
        ];
        let (level, tooltip) = tray_state(&rows, false);
        assert_eq!(level, Level::Green);
        assert!(!tooltip.contains("claude-old"));
    }

    #[test]
    fn the_tooltip_lists_one_line_per_enabled_account() {
        let rows = vec![
            (account("claude", true), Some(ok_snapshot(15, 4, &[("Fable", 5)]))),
            (account("claude3", true), Some(ok_snapshot(2, 1, &[]))),
        ];
        let (_, tooltip) = tray_state(&rows, false);
        assert_eq!(
            tooltip,
            "claude  S 15% · W 4% · Fable 5%\nclaude3  S 2% · W 1%"
        );
    }

    #[test]
    fn several_per_model_segments_are_repeated_by_label() {
        let rows = vec![(
            account("claude", true),
            Some(ok_snapshot(15, 4, &[("Fable", 5), ("Opus", 12)])),
        )];
        let (_, tooltip) = tray_state(&rows, false);
        assert_eq!(tooltip, "claude  S 15% · W 4% · Fable 5% · Opus 12%");
    }

    #[test]
    fn a_failing_account_renders_as_err() {
        let rows = vec![
            (account("claude", true), Some(ok_snapshot(15, 4, &[]))),
            (account("claude3", true), Some(failed_snapshot("spawn_error"))),
        ];
        let (_, tooltip) = tray_state(&rows, false);
        assert_eq!(tooltip, "claude  S 15% · W 4%\nclaude3  err");
    }

    #[test]
    fn an_account_with_no_snapshot_yet_renders_as_err() {
        let rows = vec![(account("claude", true), None)];
        let (_, tooltip) = tray_state(&rows, false);
        assert_eq!(tooltip, "claude  err");
    }

    #[test]
    fn an_empty_account_list_is_grey_with_an_explanatory_tooltip() {
        let (level, tooltip) = tray_state(&[], false);
        assert_eq!(level, Level::Grey);
        assert_eq!(tooltip, "no enabled accounts");
    }
}
```

- [ ] **Step 2: Run test to verify it fails** — first add `pub mod tray;` to `src-tauri/src/lib.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib tray::
```

Expect `cannot find function 'tray_state' in this scope` and `cannot find type 'Level' in this scope`.

- [ ] **Step 3: Write minimal implementation** — prepend to `src-tauri/src/tray.rs`, above the test module:

```rust
use crate::usage::{Account, SnapshotDto};

/// D9: the worst percentage across every enabled account and every window.
/// `Halted` is a distinct level that takes precedence over all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Halted,
    Grey,
    Green,
    Amber,
    Red,
}

impl Level {
    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Halted => "halted",
            Level::Grey => "grey",
            Level::Green => "green",
            Level::Amber => "amber",
            Level::Red => "red",
        }
    }

    fn from_pct(pct: u8) -> Level {
        match pct {
            0..=69 => Level::Green,
            70..=89 => Level::Amber,
            _ => Level::Red,
        }
    }
}

/// The largest percentage across every window of one `ok` snapshot.
fn worst_pct(dto: &SnapshotDto) -> Option<u8> {
    if dto.outcome != "ok" {
        return None;
    }
    let mut worst: Option<u8> = None;
    let mut consider = |p: u8| {
        worst = Some(worst.map_or(p, |w: u8| w.max(p)));
    };
    if let Some(w) = dto.session {
        consider(w.pct);
    }
    if let Some(w) = dto.week_all {
        consider(w.pct);
    }
    for m in &dto.week_models {
        consider(m.pct);
    }
    worst
}

fn tooltip_line(account: &Account, dto: Option<&SnapshotDto>) -> String {
    let dto = match dto {
        Some(d) if d.outcome == "ok" => d,
        _ => return format!("{}  err", account.label),
    };
    let mut segments: Vec<String> = Vec::new();
    if let Some(w) = dto.session {
        segments.push(format!("S {}%", w.pct));
    }
    if let Some(w) = dto.week_all {
        segments.push(format!("W {}%", w.pct));
    }
    for m in &dto.week_models {
        segments.push(format!("{} {}%", m.label, m.pct));
    }
    if segments.is_empty() {
        return format!("{}  err", account.label);
    }
    format!("{}  {}", account.label, segments.join(" · "))
}

/// Pure: level plus tooltip for the tray icon. `halted` is read from the same
/// store key `get_dashboard` reports.
pub fn tray_state(latest: &[(Account, Option<SnapshotDto>)], halted: bool) -> (Level, String) {
    if halted {
        return (Level::Halted, "polling halted — guard tripped".to_string());
    }

    let enabled: Vec<&(Account, Option<SnapshotDto>)> =
        latest.iter().filter(|(a, _)| a.enabled).collect();
    if enabled.is_empty() {
        return (Level::Grey, "no enabled accounts".to_string());
    }

    let worst = enabled
        .iter()
        .filter_map(|(_, d)| d.as_ref().and_then(worst_pct))
        .max();
    let level = match worst {
        Some(p) => Level::from_pct(p),
        None => Level::Grey,
    };

    let tooltip = enabled
        .iter()
        .map(|(a, d)| tooltip_line(a, d.as_ref()))
        .collect::<Vec<String>>()
        .join("\n");

    (level, tooltip)
}
```

- [ ] **Step 4: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib tray::
cargo clippy --all-targets -- -D warnings
```

Expect 12 passing tests, no warnings.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 16: add the pure tray state function

Computes the worst-of level across every window of every enabled account's
latest ok snapshot with the green, amber and red thresholds, gives the
global guard halt its own level that takes precedence over everything, and
builds the per-account tooltip with repeated per-model segments and an err
marker for any account whose latest outcome is not ok.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 17: Logging

**Files:** Create `src-tauri/src/logging.rs`; Modify `src-tauri/src/lib.rs`
**Interfaces:** Consumes: `AppError`, `AppResult` (Task 2). Produces: `pub const LOG_FILE_PREFIX: &str`, `pub const LOG_FILES_KEPT: usize`, `pub fn filter_for(level: &str) -> AppResult<tracing_subscriber::EnvFilter>`, `pub struct LogHandle` with `set_level(&self, level: &str) -> AppResult<()>`, `pub fn init_logging(log_dir: &Path, level: &str) -> AppResult<LogHandle>`.

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/logging.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_and_debug_are_the_only_accepted_levels() {
        assert!(filter_for("info").is_ok());
        assert!(filter_for("debug").is_ok());
    }

    #[test]
    fn an_unknown_level_is_out_of_range() {
        for bad in ["trace", "TRACE", "warn", "", "verbose"] {
            let err = filter_for(bad).expect_err("must reject");
            assert_eq!(err.code(), "out_of_range", "level {bad}");
        }
    }

    #[test]
    fn the_rotation_policy_matches_the_spec() {
        assert_eq!(LOG_FILES_KEPT, 7);
        assert_eq!(LOG_FILE_PREFIX, "claude-usage-tracker");
    }

    #[test]
    fn init_logging_creates_the_log_directory_and_writes_a_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("logs");
        let handle = init_logging(&dir, "info").expect("init");

        tracing::info!(probe = "hello", "startup probe");
        handle.set_level("debug").expect("raise level");
        tracing::debug!(probe = "hello", "debug probe");

        // The non-blocking writer flushes on its own schedule; give it a
        // moment, then assert a file exists with our prefix.
        std::thread::sleep(std::time::Duration::from_millis(500));
        drop(handle);

        assert!(dir.is_dir(), "log dir must be created");
        let files: Vec<String> = std::fs::read_dir(&dir)
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        assert!(
            files.iter().any(|f| f.starts_with(LOG_FILE_PREFIX)),
            "expected a log file, found {files:?}"
        );
    }

    #[test]
    fn setting_an_unknown_level_at_runtime_is_rejected() {
        // A second init in the same process must not install a global
        // subscriber twice; the handle is still usable for validation.
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("logs2");
        match init_logging(&dir, "info") {
            Ok(handle) => {
                let err = handle.set_level("trace").expect_err("must reject");
                assert_eq!(err.code(), "out_of_range");
            }
            Err(e) => {
                // Already initialised by the other test; that is the only
                // acceptable failure here.
                assert_eq!(e.code(), "internal");
            }
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails** — first add `pub mod logging;` to `src-tauri/src/lib.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib logging:: -- --test-threads=1
```

Expect `cannot find function 'filter_for' in this scope`.

- [ ] **Step 3: Write minimal implementation** — prepend to `src-tauri/src/logging.rs`, above the test module:

```rust
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{reload, EnvFilter, Registry};

use crate::error::{AppError, AppResult};

pub const LOG_FILE_PREFIX: &str = "claude-usage-tracker";
pub const LOG_FILES_KEPT: usize = 7;

/// D13: only two levels are user-selectable. `info` keeps milestones;
/// `debug` is the firehose, one switch away.
pub fn filter_for(level: &str) -> AppResult<EnvFilter> {
    let directive = match level {
        "info" => "info",
        "debug" => "debug",
        other => {
            return Err(AppError::OutOfRange(format!(
                "log_level must be info or debug, got {other}"
            )))
        }
    };
    EnvFilter::try_new(directive)
        .map_err(|e| AppError::Internal(format!("invalid log filter: {e}")))
}

/// Keeps the reload handle and the non-blocking writer's worker alive.
/// Dropping it flushes and stops the writer thread.
pub struct LogHandle {
    reload: reload::Handle<EnvFilter, Registry>,
    _guard: WorkerGuard,
}

impl LogHandle {
    /// Applied immediately by `set_settings`.
    pub fn set_level(&self, level: &str) -> AppResult<()> {
        let filter = filter_for(level)?;
        self.reload
            .reload(filter)
            .map_err(|e| AppError::Internal(format!("could not reload log filter: {e}")))?;
        tracing::info!(level, "log level changed");
        Ok(())
    }
}

/// JSON lines to a daily-rotating file in the app log dir, with the level
/// behind a reload handle. Returns an `internal` error if a global subscriber
/// is already installed in this process.
pub fn init_logging(log_dir: &Path, level: &str) -> AppResult<LogHandle> {
    crate::paths::ensure_dir(log_dir)?;

    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix("log")
        .max_log_files(LOG_FILES_KEPT)
        .build(log_dir)
        .map_err(|e| AppError::Io(format!("could not open log file: {e}")))?;

    let (writer, guard) = tracing_appender::non_blocking(appender);

    let (filter_layer, reload_handle) = reload::Layer::new(filter_for(level)?);
    let fmt_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_current_span(false)
        .with_span_list(false)
        .with_target(true)
        .with_writer(writer);

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .try_init()
        .map_err(|e| AppError::Internal(format!("logging already initialised: {e}")))?;

    tracing::info!(
        log_dir = %log_dir.display(),
        level,
        files_kept = LOG_FILES_KEPT,
        "logging initialised"
    );

    Ok(LogHandle {
        reload: reload_handle,
        _guard: guard,
    })
}
```

- [ ] **Step 4: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib logging:: -- --test-threads=1
cargo clippy --all-targets -- -D warnings
```

Expect 5 passing tests, no warnings. The single test thread matters because a global subscriber can only be installed once per process.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 17: add JSON logging with daily rotation and a runtime level switch

Installs a JSON formatting layer over a daily-rotating appender keeping
seven files in the app log dir, with the level filter behind a reload
handle so set_settings can raise the firehose without a restart. Only info
and debug are accepted; anything else is out_of_range.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 18: Login terminal

**Files:** Create `src-tauri/src/login.rs`; Modify `src-tauri/src/lib.rs`
**Interfaces:** Consumes: `AppError`, `AppResult` (Task 2), `paths::{login_script_dir, empty_dir}` (Task 6). Produces: `pub fn shell_single_quote(s: &str) -> String`, `pub fn windows_script(binary: &Path, config_dir: &Path) -> String`, `pub fn unix_script(binary: &Path, config_dir: &Path) -> String`, `pub fn script_file_name() -> &'static str`, `pub fn write_login_script(app_data_dir: &Path, binary: &Path, config_dir: &Path) -> AppResult<PathBuf>`, `pub fn open_terminal_for_login(app_data_dir: &Path, binary: &Path, config_dir: &Path) -> AppResult<()>`.

- [ ] **Step 1: Write the failing test** — create `src-tauri/src/login.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn single_quoting_escapes_embedded_single_quotes() {
        assert_eq!(shell_single_quote("/home/josh"), "'/home/josh'");
        assert_eq!(
            shell_single_quote("/home/jo'sh/.claude"),
            r#"'/home/jo'\''sh/.claude'"#
        );
        assert_eq!(shell_single_quote(""), "''");
    }

    #[test]
    fn the_windows_script_sets_the_env_var_with_the_quoted_set_form() {
        let s = windows_script(
            &PathBuf::from(r"C:\Users\josh\.local\bin\claude.exe"),
            &PathBuf::from(r"C:\Users\josh\.claude3"),
        );
        assert!(s.starts_with("@echo off\r\n"));
        assert!(s.contains("set \"CLAUDE_CONFIG_DIR=C:\\Users\\josh\\.claude3\"\r\n"));
        assert!(s.contains("\"C:\\Users\\josh\\.local\\bin\\claude.exe\" /login\r\n"));
        assert!(s.trim_end().ends_with("pause"));
    }

    #[test]
    fn the_windows_script_takes_ampersands_and_percents_literally() {
        let s = windows_script(
            &PathBuf::from(r"C:\bin\claude.exe"),
            &PathBuf::from(r"C:\Users\a&b %USERNAME%\.claude"),
        );
        assert!(
            s.contains(r#"set "CLAUDE_CONFIG_DIR=C:\Users\a&b %USERNAME%\.claude""#),
            "the quoted set form takes the value literally: {s}"
        );
    }

    #[test]
    fn the_unix_script_single_quotes_both_paths() {
        let s = unix_script(
            &PathBuf::from("/home/josh/.local/bin/claude"),
            &PathBuf::from("/home/josh/.claude3"),
        );
        assert!(s.starts_with("#!/bin/bash\n"));
        assert!(s.contains("export CLAUDE_CONFIG_DIR='/home/josh/.claude3'\n"));
        assert!(s.contains("exec '/home/josh/.local/bin/claude' /login\n"));
    }

    #[test]
    fn the_unix_script_survives_a_path_with_a_space_and_a_dollar_sign() {
        let s = unix_script(
            &PathBuf::from("/home/josh/my bin/claude"),
            &PathBuf::from("/home/josh/$HOME dir/.claude"),
        );
        assert!(s.contains("export CLAUDE_CONFIG_DIR='/home/josh/$HOME dir/.claude'"));
        assert!(s.contains("exec '/home/josh/my bin/claude' /login"));
    }

    #[test]
    fn writing_the_script_empties_the_directory_first() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = crate::paths::login_script_dir(tmp.path());
        crate::paths::ensure_dir(&dir).expect("mkdir");
        std::fs::write(dir.join("stale.txt"), "old").expect("write stale");

        let binary = tmp.path().join("claude");
        let config = tmp.path().join(".claude3");
        std::fs::create_dir_all(&config).expect("cfg");
        std::fs::write(&binary, "x").expect("bin");

        let script = write_login_script(tmp.path(), &binary, &config).expect("write");
        assert!(script.is_file());
        assert!(!dir.join("stale.txt").exists(), "stale files are cleared");
        assert_eq!(
            script.file_name().and_then(|n| n.to_str()),
            Some(script_file_name())
        );
    }

    #[test]
    fn writing_the_script_overwrites_the_previous_one() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let binary = tmp.path().join("claude");
        let first = tmp.path().join(".claude");
        let second = tmp.path().join(".claude3");
        std::fs::create_dir_all(&first).expect("cfg1");
        std::fs::create_dir_all(&second).expect("cfg2");
        std::fs::write(&binary, "x").expect("bin");

        write_login_script(tmp.path(), &binary, &first).expect("first");
        let path = write_login_script(tmp.path(), &binary, &second).expect("second");
        let body = std::fs::read_to_string(&path).expect("read");
        assert!(body.contains(".claude3"));
        assert!(!body.contains("CLAUDE_CONFIG_DIR=") || body.matches("CLAUDE_CONFIG_DIR").count() == 1);
    }

    #[cfg(windows)]
    #[test]
    fn a_double_quote_in_the_script_path_is_rejected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bad = tmp.path().join("we\"ird");
        // The rejection is about the resolved script path, so test the guard
        // directly rather than trying to create such a directory.
        let err = reject_quoted_path(&bad).expect_err("must reject");
        assert_eq!(err.code(), "terminal_unavailable");
    }

    #[test]
    fn a_clean_script_path_passes_the_quote_guard() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(reject_quoted_path(&tmp.path().join("login.cmd")).is_ok());
    }

    #[test]
    fn a_missing_config_dir_is_not_found() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let binary = tmp.path().join("claude");
        std::fs::write(&binary, "x").expect("bin");
        let err = write_login_script(tmp.path(), &binary, &tmp.path().join("nope"))
            .expect_err("must reject");
        assert_eq!(err.code(), "not_found");
    }
}
```

- [ ] **Step 2: Run test to verify it fails** — first add `pub mod login;` to `src-tauri/src/lib.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib login::
```

Expect `cannot find function 'shell_single_quote' in this scope`.

- [ ] **Step 3: Write minimal implementation** — prepend to `src-tauri/src/login.rs`, above the test module:

```rust
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::error::{AppError, AppResult};
use crate::paths::{empty_dir, login_script_dir};

/// POSIX single-quoting: everything inside is literal, and an embedded quote
/// is closed, escaped and reopened.
pub fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn script_file_name() -> &'static str {
    if cfg!(windows) {
        "login.cmd"
    } else if cfg!(target_os = "macos") {
        "login.command"
    } else {
        "login.sh"
    }
}

/// cmd's `set "K=V"` form takes the value literally, including `&` and `%`,
/// and the binary is invoked directly by cmd rather than through a second
/// shell, so the value is never expanded again.
pub fn windows_script(binary: &Path, config_dir: &Path) -> String {
    format!(
        "@echo off\r\nset \"CLAUDE_CONFIG_DIR={}\"\r\n\"{}\" /login\r\npause\r\n",
        config_dir.display(),
        binary.display()
    )
}

pub fn unix_script(binary: &Path, config_dir: &Path) -> String {
    format!(
        "#!/bin/bash\nexport CLAUDE_CONFIG_DIR={}\nexec {} /login\n",
        shell_single_quote(&config_dir.to_string_lossy()),
        shell_single_quote(&binary.to_string_lossy())
    )
}

/// Rust's automatic quoting only quotes when it sees a space, and cmd would
/// split an unquoted `&`, so the script path is always explicitly quoted. A
/// path containing a double quote cannot be quoted safely, so it is refused.
pub fn reject_quoted_path(path: &Path) -> AppResult<()> {
    if path.to_string_lossy().contains('"') {
        return Err(AppError::TerminalUnavailable(format!(
            "script path contains a double quote: {}",
            path.display()
        )));
    }
    Ok(())
}

/// Writes the login helper into `<app_data_dir>/login/`, clearing the
/// directory first so nothing accumulates (spec 6.8).
pub fn write_login_script(
    app_data_dir: &Path,
    binary: &Path,
    config_dir: &Path,
) -> AppResult<PathBuf> {
    if !config_dir.is_dir() {
        return Err(AppError::NotFound(format!(
            "no such config directory: {}",
            config_dir.display()
        )));
    }

    let dir = login_script_dir(app_data_dir);
    empty_dir(&dir)?;

    let path = dir.join(script_file_name());
    reject_quoted_path(&path)?;

    let body = if cfg!(windows) {
        windows_script(binary, config_dir)
    } else {
        unix_script(binary, config_dir)
    };
    std::fs::write(&path, body)
        .map_err(|e| AppError::Io(format!("could not write {}: {e}", path.display())))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| AppError::Io(format!("could not chmod {}: {e}", path.display())))?;
    }

    Ok(path)
}

#[cfg(windows)]
fn launch(script: &Path) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    let attempted = format!("cmd.exe /s /c start \"\" cmd.exe /k \"{}\"", script.display());
    std::process::Command::new("cmd.exe")
        .raw_arg(format!(
            "/s /c \"start \"\" cmd.exe /k \"{}\"\"",
            script.display()
        ))
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            warn!(error = %e, "could not open a login terminal");
            AppError::TerminalUnavailable(attempted)
        })
}

#[cfg(target_os = "macos")]
fn launch(script: &Path) -> AppResult<()> {
    let attempted = format!("open {}", script.display());
    std::process::Command::new("open")
        .arg(script)
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            warn!(error = %e, "could not open a login terminal");
            AppError::TerminalUnavailable(attempted)
        })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn launch(script: &Path) -> AppResult<()> {
    let candidates: [(&str, &str); 4] = [
        ("gnome-terminal", "--"),
        ("konsole", "-e"),
        ("xfce4-terminal", "-e"),
        ("xterm", "-e"),
    ];
    let mut attempted: Vec<String> = Vec::new();
    for (program, flag) in candidates {
        attempted.push(format!("{program} {flag} {}", script.display()));
        if std::process::Command::new(program)
            .arg(flag)
            .arg(script)
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }
    warn!(attempted = ?attempted, "no terminal emulator could be launched");
    Err(AppError::TerminalUnavailable(attempted.join("; ")))
}

/// Opens a visible terminal running `<binary> /login` with `CLAUDE_CONFIG_DIR`
/// set. Paths are never interpolated into a shell command line: they go into
/// a script file the app writes and controls.
pub fn open_terminal_for_login(
    app_data_dir: &Path,
    binary: &Path,
    config_dir: &Path,
) -> AppResult<()> {
    let script = write_login_script(app_data_dir, binary, config_dir)?;
    info!(script = %script.display(), config_dir = %config_dir.display(), "opening login terminal");
    launch(&script)
}
```

- [ ] **Step 4: Run tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib login::
cargo clippy --all-targets -- -D warnings
```

Expect 10 passing tests on Windows (9 elsewhere), no warnings.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 18: add the login terminal helper

Writes a per-OS script into an app-controlled directory that is emptied on
every use, so a config dir or binary path containing spaces, ampersands,
percent signs or dollar signs can never break out of a command line. cmd
gets the quoted set form and an explicitly quoted script path; POSIX gets
single-quoted paths. A path containing a double quote is refused.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 19: App state, triggers and the command surface

**Files:** Create `src-tauri/src/scheduler/triggers.rs`, `src-tauri/src/commands.rs`; Modify `src-tauri/src/scheduler/mod.rs`, `src-tauri/src/lib.rs`
**Interfaces:** Consumes: `Store` (Tasks 8–11), `DriverStatus` / `SkipReason` / `preview_manual` (Task 12), `UserSettings` / `validate_settings` / `polling_relevant_changed` (Task 9), `discovery::find_claude_binary` (Task 7), `login::open_terminal_for_login` (Task 18), `logging::LogHandle` (Task 17). Produces: `pub struct Triggers` with `manual()`, `startup()`, `account_changed(Vec<String>)`, `notified_manual()`, `notified_startup()`, `notified_changed()`, `take_changed() -> Vec<String>`; `pub type BinarySlot`; `pub fn lock_status` / `lock_binary`; `pub async fn blocking<T, F>(F) -> AppResult<T>` (the shared `spawn_blocking` hop, also used by the driver in Task 20); `pub struct Core` and every `core_*` function; the thirteen `#[tauri::command]` wrappers named in spec §8.

**v5 rule enforced throughout this task:** commands never touch `Machine`. Every piece of scheduler state they need — `gate`, `busy`, `stalled_at`, `backoff_until` — is read from the `Arc<Mutex<DriverStatus>>` snapshot the driver publishes (spec §5.1). `Core` therefore holds no machine handle at all, and backoff resets happen where the spec puts them: inside `decide` for `Manual` / `AccountChanged`, and inside the driver's settings-watch arm for a polling-relevant settings change.

- [ ] **Step 1: Write the failing test for triggers** — create `src-tauri/src/scheduler/triggers.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn five_manual_clicks_coalesce_into_one_pending_trigger() {
        let t = Arc::new(Triggers::new());
        for _ in 0..5 {
            t.manual();
        }
        // The first wait resolves immediately.
        tokio::time::timeout(Duration::from_millis(200), t.notified_manual())
            .await
            .expect("first notification");
        // There is no second pending notification.
        let second = tokio::time::timeout(Duration::from_millis(200), t.notified_manual()).await;
        assert!(second.is_err(), "clicks must coalesce, not queue");
    }

    #[tokio::test]
    async fn account_changed_ids_accumulate_and_drain_together() {
        let t = Arc::new(Triggers::new());
        t.account_changed(vec!["a".into()]);
        t.account_changed(vec!["b".into(), "a".into()]);

        tokio::time::timeout(Duration::from_millis(200), t.notified_changed())
            .await
            .expect("notification");

        let mut drained = t.take_changed();
        drained.sort();
        assert_eq!(drained, vec!["a".to_string(), "b".to_string()]);
        assert!(t.take_changed().is_empty(), "draining clears the set");
    }

    #[tokio::test]
    async fn startup_and_manual_are_separate_channels() {
        let t = Arc::new(Triggers::new());
        t.startup();
        tokio::time::timeout(Duration::from_millis(200), t.notified_startup())
            .await
            .expect("startup notification");
        let manual = tokio::time::timeout(Duration::from_millis(200), t.notified_manual()).await;
        assert!(manual.is_err(), "startup must not fire the manual channel");
    }
}
```

- [ ] **Step 2: Run test to verify it fails** — first add `pub mod triggers;` to `src-tauri/src/scheduler/mod.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib scheduler::triggers::
```

Expect `cannot find type 'Triggers' in this scope`.

- [ ] **Step 3: Write the triggers implementation** — prepend to `src-tauri/src/scheduler/triggers.rs`, above the test module:

```rust
use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};
use tokio::sync::Notify;

/// Spec 6.5: triggers reach the driver through a `Notify` per kind, never an
/// unbounded queue, so five Refresh clicks coalesce into at most one pending
/// trigger. `AccountChanged` additionally accumulates ids so enabling two
/// accounts in quick succession polls both.
#[derive(Debug, Default)]
pub struct Triggers {
    manual: Notify,
    startup: Notify,
    changed: Notify,
    changed_ids: Mutex<HashSet<String>>,
}

impl Triggers {
    pub fn new() -> Triggers {
        Triggers::default()
    }

    pub fn manual(&self) {
        self.manual.notify_one();
    }

    pub fn startup(&self) {
        self.startup.notify_one();
    }

    pub fn account_changed(&self, ids: Vec<String>) {
        {
            let mut set = self
                .changed_ids
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            set.extend(ids);
        }
        self.changed.notify_one();
    }

    pub async fn notified_manual(&self) {
        self.manual.notified().await;
    }

    pub async fn notified_startup(&self) {
        self.startup.notified().await;
    }

    pub async fn notified_changed(&self) {
        self.changed.notified().await;
    }

    /// Drains every accumulated id into one decision.
    pub fn take_changed(&self) -> Vec<String> {
        let mut set = self
            .changed_ids
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        set.drain().collect()
    }
}
```

- [ ] **Step 4: Write the failing test for the command core** — create `src-tauri/src/commands.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::machine::Gate;
    use crate::store::settings::UserSettings;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn defaults() -> UserSettings {
        UserSettings {
            interval_secs: 60,
            timeout_secs: 30,
            claude_binary: String::new(),
            close_to_tray: true,
            launch_at_login: false,
            log_level: "info".to_string(),
        }
    }

    fn core() -> (tempfile::TempDir, Arc<Core>) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_in_memory().expect("open"));
        store.save_settings(&defaults()).expect("seed settings");
        let (settings_tx, _rx) = tokio::sync::watch::channel(defaults());
        let core = Arc::new(Core {
            store,
            triggers: Arc::new(Triggers::new()),
            status: Arc::new(std::sync::Mutex::new(DriverStatus::default())),
            binary: Arc::new(std::sync::Mutex::new(None)),
            settings_tx,
            log: None,
            app_data_dir: tmp.path().to_path_buf(),
            log_dir: tmp.path().join("logs"),
        });
        (tmp, core)
    }

    /// Pretend the driver found a binary at its last check.
    fn with_binary(core: &Core) {
        *lock_binary(&core.binary) = Some(("/bin/claude".to_string(), "path"));
    }

    fn make_dir(root: &std::path::Path, name: &str) -> std::path::PathBuf {
        let d = root.join(name);
        std::fs::create_dir_all(&d).expect("mkdir");
        std::fs::write(d.join("settings.json"), "{}").expect("marker");
        d
    }

    #[test]
    fn add_account_rejects_a_missing_directory() {
        let (tmp, core) = core();
        let err = core_add_account(&core, &tmp.path().join("nope"), 1)
            .expect_err("must reject");
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn add_account_rejects_a_duplicate() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        core_add_account(&core, &d, 1).expect("first");
        let err = core_add_account(&core, &d, 2).expect_err("must reject");
        assert_eq!(err.code(), "duplicate");
    }

    #[test]
    fn adding_an_account_queues_an_account_changed_trigger() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &d, 1).expect("add");
        assert_eq!(core.triggers.take_changed(), vec![a.id]);
    }

    #[test]
    fn enabling_an_account_clears_the_reason_and_queues_a_trigger() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &d, 1).expect("add");
        let _ = core.triggers.take_changed();

        let off = core_update_account(&core, &a.id, None, Some(false)).expect("off");
        assert_eq!(off.disabled_reason, Some(DisabledReason::User));
        assert!(
            core.triggers.take_changed().is_empty(),
            "disabling must not queue a poll"
        );

        let on = core_update_account(&core, &a.id, None, Some(true)).expect("on");
        assert_eq!(on.disabled_reason, None);
        assert_eq!(core.triggers.take_changed(), vec![a.id]);
    }

    #[test]
    fn set_settings_accepts_the_boundary_values() {
        let (_tmp, core) = core();
        for (interval, timeout) in [(10u32, 5u32), (3600, 120)] {
            let mut s = defaults();
            s.interval_secs = interval;
            s.timeout_secs = timeout;
            core_set_settings(&core, &s).unwrap_or_else(|e| panic!("{interval}/{timeout}: {e}"));
        }
        assert_eq!(core.store.stored_settings().expect("read").interval_secs, 3600);
    }

    #[test]
    fn set_settings_rejects_one_off_values_and_keeps_the_previous() {
        let (_tmp, core) = core();
        for (interval, timeout) in [(9u32, 30u32), (3601, 30), (60, 4), (60, 121)] {
            let mut s = defaults();
            s.interval_secs = interval;
            s.timeout_secs = timeout;
            let err = core_set_settings(&core, &s).expect_err("must reject");
            assert_eq!(err.code(), "out_of_range");
        }
        assert_eq!(core.store.stored_settings().expect("read").interval_secs, 60);
    }

    #[test]
    fn a_polling_relevant_change_publishes_on_the_watch() {
        for mutate in [
            (|s: &mut UserSettings| s.interval_secs = 120) as fn(&mut UserSettings),
            |s: &mut UserSettings| s.timeout_secs = 45,
            |s: &mut UserSettings| s.claude_binary = "C:/bin/claude.exe".into(),
        ] {
            let (_tmp, core) = core();
            let mut rx = core.settings_tx.subscribe();
            let mut next = defaults();
            mutate(&mut next);

            core_set_settings(&core, &next).expect("set");

            assert!(
                rx.has_changed().unwrap_or(false),
                "a polling-relevant change must fire the watch"
            );
            assert_eq!(&*rx.borrow_and_update(), &next);
        }
    }

    #[test]
    fn the_other_three_keys_are_saved_without_touching_the_watch() {
        for mutate in [
            (|s: &mut UserSettings| s.close_to_tray = false) as fn(&mut UserSettings),
            |s: &mut UserSettings| s.launch_at_login = true,
            |s: &mut UserSettings| s.log_level = "debug".into(),
        ] {
            let (_tmp, core) = core();
            let mut rx = core.settings_tx.subscribe();
            let mut next = defaults();
            mutate(&mut next);

            core_set_settings(&core, &next).expect("set");

            assert!(
                !rx.has_changed().unwrap_or(false),
                "this key must not reach the scheduler"
            );
        }

        // ...and the value really was persisted.
        let (_tmp, core) = core();
        let mut next = defaults();
        next.log_level = "debug".into();
        next.close_to_tray = false;
        core_set_settings(&core, &next).expect("set");
        let stored = core.store.stored_settings().expect("read");
        assert_eq!(stored.log_level, "debug");
        assert!(!stored.close_to_tray);
    }

    #[test]
    fn saving_identical_settings_does_not_disturb_the_scheduler() {
        let (_tmp, core) = core();
        let mut rx = core.settings_tx.subscribe();
        core_set_settings(&core, &defaults()).expect("set");
        assert!(!rx.has_changed().unwrap_or(false));
    }

    #[test]
    fn set_settings_never_touches_the_halt_flag() {
        let (_tmp, core) = core();
        core.store
            .set_polling_halted("guard_tripped:1")
            .expect("halt");
        core_set_settings(&core, &defaults()).expect("set");
        assert_eq!(
            core.store.polling_halted().expect("read"),
            Some("guard_tripped:1".to_string())
        );
    }

    #[test]
    fn clear_halt_clears_the_flag_and_does_not_poll() {
        let (_tmp, core) = core();
        core.store
            .set_polling_halted("guard_tripped:1700000000000")
            .expect("halt");

        core_clear_halt(&core).expect("clear");

        assert_eq!(core.store.polling_halted().expect("read"), None);
        assert!(
            core.triggers.take_changed().is_empty(),
            "clear_halt must not queue an account-changed poll"
        );
        // Every quota-spending action stays a separate, explicit act: the
        // manual channel must have nothing pending.
        let pending = futures_lite_poll_once(&core.triggers);
        assert!(!pending, "clear_halt must not queue a manual poll");
    }

    /// True if a manual notification is already pending.
    fn futures_lite_poll_once(t: &Triggers) -> bool {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime");
        rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(50), t.notified_manual())
                .await
                .is_ok()
        })
    }

    #[test]
    fn poll_now_reports_the_skip_reason_when_halted() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        core_add_account(&core, &d, 1).expect("add");
        core.store.set_polling_halted("guard_tripped:1").expect("halt");
        with_binary(&core);
        assert_eq!(core_poll_now(&core).expect("poll"), "skipped:halted");
    }

    #[test]
    fn poll_now_reports_no_binary_and_no_enabled_accounts() {
        let (tmp, core) = core();
        assert_eq!(core_poll_now(&core).expect("poll"), "skipped:no_binary");

        with_binary(&core);
        assert_eq!(
            core_poll_now(&core).expect("poll"),
            "skipped:no_enabled_accounts"
        );

        let d = make_dir(tmp.path(), ".claude3");
        core_add_account(&core, &d, 1).expect("add");
        assert_eq!(core_poll_now(&core).expect("poll"), "started");
    }

    #[test]
    fn poll_now_reports_busy_from_the_published_snapshot() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        core_add_account(&core, &d, 1).expect("add");
        with_binary(&core);
        lock_status(&core.status).busy = true;
        assert_eq!(core_poll_now(&core).expect("poll"), "skipped:busy");
    }

    #[test]
    fn the_dashboard_copies_the_published_snapshot() {
        let (tmp, core) = core();
        let dir = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &dir, 1).expect("add");
        core.store
            .insert_snapshot(&a.id, 1000, &PollOutcome::Timeout(30), Some("raw"), 5)
            .expect("snapshot");
        *lock_binary(&core.binary) = Some(("/bin/claude".to_string(), "local_bin"));
        {
            let mut st = lock_status(&core.status);
            st.gate = Gate::Active;
            st.busy = true;
            st.stalled_at = Some(4242);
        }

        let dash = core_get_dashboard(&core).expect("dashboard");
        assert_eq!(dash.gate, "active");
        assert!(dash.busy);
        assert_eq!(dash.halted, None);
        assert_eq!(dash.stalled_at, Some(4242));
        assert_eq!(dash.binary.path.as_deref(), Some("/bin/claude"));
        assert_eq!(dash.binary.source, Some("local_bin"));
        assert_eq!(dash.interval_secs, 60);
        assert_eq!(dash.accounts.len(), 1);
        assert_eq!(
            dash.accounts[0].latest.as_ref().map(|s| s.outcome),
            Some("timeout")
        );
    }

    #[test]
    fn the_dashboard_reports_the_backoff_deadline_from_the_snapshot() {
        let (tmp, core) = core();
        let dir = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &dir, 1).expect("add");

        let mut backoff = HashMap::new();
        backoff.insert(a.id.clone(), 1000 + 60_000);
        lock_status(&core.status).backoff_until = backoff;

        let dash = core_get_dashboard(&core).expect("dashboard");
        assert_eq!(dash.accounts[0].backoff_until, Some(1000 + 60_000));
    }

    #[test]
    fn an_account_with_no_entry_in_the_snapshot_has_no_backoff() {
        let (tmp, core) = core();
        let dir = make_dir(tmp.path(), ".claude3");
        core_add_account(&core, &dir, 1).expect("add");
        let dash = core_get_dashboard(&core).expect("dashboard");
        assert_eq!(dash.accounts[0].backoff_until, None);
    }

    #[test]
    fn removing_an_account_cascades_and_rescan_adds_disabled_rows() {
        let (tmp, core) = core();
        let one = make_dir(tmp.path(), ".claude");
        let two = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &one, 1).expect("add");

        let added = core_rescan_profiles(&core, tmp.path(), 2).expect("rescan");
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].config_dir, dunce::canonicalize(&two).unwrap_or(two));
        assert!(!added[0].enabled);

        core_remove_account(&core, &a.id).expect("remove");
        assert_eq!(core.store.list_accounts().expect("list").len(), 1);
    }

    #[test]
    fn get_snapshot_raw_returns_raw_and_error() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &d, 1).expect("add");
        let id = core
            .store
            .insert_snapshot(
                &a.id,
                1,
                &PollOutcome::ParseError("missing session line".into()),
                Some("the raw report"),
                7,
            )
            .expect("snapshot");

        let got = core_get_snapshot_raw(&core, id).expect("raw");
        assert_eq!(got.raw.as_deref(), Some("the raw report"));
        assert_eq!(got.error.as_deref(), Some("missing session line"));
    }

    #[test]
    fn get_history_returns_hourly_points() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &d, 1).expect("add");
        let hour = 3_600_000i64;
        let base = 100 * hour;
        for (t, pct) in [(base + 1000, 4u8), (base + 2000, 9), (base + hour, 6)] {
            let outcome = PollOutcome::Ok(crate::usage::Parsed {
                session: crate::usage::Window { pct: 1, resets_at: None },
                week_all: crate::usage::Window { pct, resets_at: None },
                week_models: vec![],
            });
            core.store
                .insert_snapshot(&a.id, t, &outcome, None, 1)
                .expect("snapshot");
        }

        let points = core_get_history(&core, &a.id, base + hour * 8).expect("history");
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].pct, 9);
        assert_eq!(points[1].pct, 6);
    }
}
```

- [ ] **Step 5: Run test to verify it fails** — first add `pub mod commands;` to `src-tauri/src/lib.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib commands::
```

Expect `cannot find type 'Core' in this scope` and `cannot find function 'core_get_dashboard' in this scope`.

- [ ] **Step 6: Write the state and DTO types** — prepend to `src-tauri/src/commands.rs`, above the test module:

```rust
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::discovery::{enumerate_profiles, find_claude_binary};
use crate::error::{AppError, AppResult};
use crate::logging::LogHandle;
use crate::scheduler::machine::{preview_manual, DriverStatus};
use crate::scheduler::triggers::Triggers;
use crate::store::settings::{polling_relevant_changed, validate_settings, UserSettings};
use crate::store::{HistoryPoint, Store};
use crate::usage::{Account, DisabledReason, PollOutcome, SnapshotDto};

/// `(path, source)` of the binary found at the driver's last check. Kept
/// beside `DriverStatus` rather than inside it because spec 5.1 defines
/// `DriverStatus` as scheduler state only.
pub type BinarySlot = Arc<Mutex<Option<(String, &'static str)>>>;

pub fn lock_status(s: &Mutex<DriverStatus>) -> MutexGuard<'_, DriverStatus> {
    s.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn lock_binary(
    b: &Mutex<Option<(String, &'static str)>>,
) -> MutexGuard<'_, Option<(String, &'static str)>> {
    b.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Everything a command needs, with no Tauri types, so the whole surface is
/// unit-testable. The `#[tauri::command]` wrappers are thin. There is
/// deliberately no `Machine` handle here: scheduler state is read only from
/// the published `DriverStatus` snapshot.
pub struct Core {
    pub store: Arc<Store>,
    pub triggers: Arc<Triggers>,
    pub status: Arc<Mutex<DriverStatus>>,
    pub binary: BinarySlot,
    pub settings_tx: watch::Sender<UserSettings>,
    pub log: Option<Arc<LogHandle>>,
    pub app_data_dir: PathBuf,
    pub log_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct BinaryInfo {
    pub path: Option<String>,
    pub source: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountRow {
    pub account: Account,
    pub latest: Option<SnapshotDto>,
    pub backoff_until: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dashboard {
    pub accounts: Vec<AccountRow>,
    pub gate: &'static str,
    pub busy: bool,
    pub halted: Option<String>,
    pub stalled_at: Option<i64>,
    pub binary: BinaryInfo,
    pub interval_secs: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawSnapshot {
    pub raw: Option<String>,
    pub error: Option<String>,
}

/// Spec 6.6: every `Store` method is synchronous and the connection mutex is
/// never held across an `await`, so every async caller hops to the blocking
/// pool first. `busy_timeout=5000` means a contended statement can park a
/// thread for five seconds, which must never be a runtime worker.
///
/// Public because the scheduler driver uses the same helper for its own store
/// access; it is the single place that hop is expressed.
pub async fn blocking<T, F>(f: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Internal(format!("blocking task failed: {e}")))?
}
```

- [ ] **Step 7: Write the command core functions** — append to the same section of `src-tauri/src/commands.rs`, still above the test module:

```rust
/// Cheap; called on every `usage:updated` (debounced in the frontend).
/// Copies the published `DriverStatus` and never reads `Machine` (spec 5.1).
pub fn core_get_dashboard(core: &Core) -> AppResult<Dashboard> {
    let accounts = core.store.list_accounts()?;
    let latest = core.store.latest_per_account()?;
    let settings = core.store.stored_settings()?;
    let halted = core.store.polling_halted()?;

    let status = lock_status(&core.status).clone();
    let binary = {
        let found = lock_binary(&core.binary);
        BinaryInfo {
            path: found.as_ref().map(|(p, _)| p.clone()),
            source: found.as_ref().map(|(_, s)| *s),
        }
    };

    let rows = accounts
        .into_iter()
        .map(|account| AccountRow {
            latest: latest.get(&account.id).cloned(),
            backoff_until: status.backoff_until.get(&account.id).copied(),
            account,
        })
        .collect();

    Ok(Dashboard {
        accounts: rows,
        gate: status.gate.as_str(),
        busy: status.busy,
        halted,
        stalled_at: status.stalled_at,
        binary,
        interval_secs: settings.interval_secs,
    })
}

pub fn core_get_history(core: &Core, account_id: &str, now: i64) -> AppResult<Vec<HistoryPoint>> {
    let since = now - 7 * 24 * 60 * 60 * 1000;
    core.store.history(account_id, since)
}

/// `"started"` or `"skipped:<reason>"`. The preview reads the published
/// snapshot, and it is exact because a Manual trigger bypasses both the gate
/// and backoff.
pub fn core_poll_now(core: &Core) -> AppResult<String> {
    let halted = core.store.polling_halted()?.is_some();
    let binary_present = lock_binary(&core.binary).is_some();
    let enabled = core.store.enabled_account_ids()?;
    let status = lock_status(&core.status).clone();

    match preview_manual(&status, binary_present, halted, &enabled) {
        Some(reason) => {
            info!(reason = reason.as_str(), "manual poll skipped");
            Ok(format!("skipped:{}", reason.as_str()))
        }
        None => {
            core.triggers.manual();
            Ok("started".to_string())
        }
    }
}

pub fn core_add_account(core: &Core, config_dir: &Path, now: i64) -> AppResult<Account> {
    let account = core.store.add_account(config_dir, true, None, false, now)?;
    info!(account_id = %account.id, label = %account.label, "account added");
    core.triggers.account_changed(vec![account.id.clone()]);
    Ok(account)
}

/// `enabled: true` clears `disabled_reason` and triggers `AccountChanged`;
/// `enabled: false` sets the reason to `user` and polls nothing.
pub fn core_update_account(
    core: &Core,
    id: &str,
    label: Option<&str>,
    enabled: Option<bool>,
) -> AppResult<Account> {
    let account = core.store.update_account(id, label, enabled)?;
    // The backoff reset for this account happens inside `decide` when the
    // AccountChanged trigger is consumed; commands never touch the machine.
    if enabled == Some(true) {
        core.triggers.account_changed(vec![account.id.clone()]);
    }
    info!(account_id = %account.id, label = %account.label, enabled = account.enabled, "account updated");
    Ok(account)
}

pub fn core_remove_account(core: &Core, id: &str) -> AppResult<()> {
    core.store.remove_account(id)?;
    info!(account_id = id, "account removed");
    Ok(())
}

/// Newly added accounts come back disabled (spec 6.1).
pub fn core_rescan_profiles(core: &Core, home: &Path, now: i64) -> AppResult<Vec<Account>> {
    let candidates = enumerate_profiles(home);
    let added = core.store.rescan_accounts(&candidates, now)?;
    info!(
        scanned = candidates.len(),
        added = added.len(),
        "profile rescan"
    );
    Ok(added)
}

/// `launch_at_login` is supplied by the caller from the autostart plugin.
pub fn core_get_settings(core: &Core, launch_at_login: bool) -> AppResult<UserSettings> {
    let mut s = core.store.stored_settings()?;
    s.launch_at_login = launch_at_login;
    Ok(s)
}

/// Validates, saves, and then applies the change in two distinct ways
/// (spec §8, D16):
///
/// * `interval_secs`, `timeout_secs` and `claude_binary` are polling-relevant,
///   so a change to any of them publishes on the settings watch. The driver's
///   watch arm is what moves the deadline and resets backoff — this function
///   does neither itself, because it has no machine handle.
/// * `close_to_tray`, `launch_at_login` and `log_level` are applied directly
///   and must never touch the scheduler. `log_level` goes through the reload
///   handle here; `launch_at_login` is written to the autostart plugin by the
///   Tauri wrapper; `close_to_tray` is simply read from the store when a
///   window close arrives.
///
/// Never touches `polling_halted`.
pub fn core_set_settings(core: &Core, next: &UserSettings) -> AppResult<()> {
    validate_settings(next)?;
    let previous = core.store.stored_settings()?;
    core.store.save_settings(next)?;

    if previous.log_level != next.log_level {
        if let Some(log) = core.log.as_ref() {
            log.set_level(&next.log_level)?;
        }
    }

    let scheduler_affected = polling_relevant_changed(&previous, next);
    if scheduler_affected && core.settings_tx.send(next.clone()).is_err() {
        warn!("settings watch has no receiver; the driver may not be running");
    }

    info!(
        interval_secs = next.interval_secs,
        timeout_secs = next.timeout_secs,
        log_level = %next.log_level,
        scheduler_affected,
        "settings updated"
    );
    Ok(())
}

/// Clears the flag and logs the previous value at WARN. Does **not** poll:
/// every quota-spending action stays a separate, explicit act.
pub fn core_clear_halt(core: &Core) -> AppResult<()> {
    let previous = core.store.clear_polling_halted()?;
    warn!(previous = ?previous, "polling halt cleared by the user");
    Ok(())
}

pub fn core_open_login(core: &Core, id: &str) -> AppResult<()> {
    let account = core
        .store
        .account_by_id(id)?
        .ok_or_else(|| AppError::NotFound(format!("no such account: {id}")))?;
    let override_path = core.store.stored_settings()?.claude_binary;
    let found = find_claude_binary(Some(&override_path)).ok_or_else(|| {
        AppError::NotFound("claude binary not found; set it in Settings".to_string())
    })?;
    crate::login::open_terminal_for_login(&core.app_data_dir, &found.path, &account.config_dir)
}

pub fn core_get_snapshot_raw(core: &Core, snapshot_id: i64) -> AppResult<RawSnapshot> {
    let (raw, error) = core.store.snapshot_raw(snapshot_id)?;
    Ok(RawSnapshot { raw, error })
}
```

- [ ] **Step 8: Run the core tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib commands::
```

Expect 20 passing tests.

- [ ] **Step 9: Write the Tauri command wrappers** — append to `src-tauri/src/commands.rs`, still above the test module. Each wrapper does its blocking work on the blocking pool so the connection mutex is never held across an `await`:

```rust
use tauri::{Manager, State};
use tauri_plugin_autostart::ManagerExt;

pub type SharedCore = Arc<Core>;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[tauri::command]
pub async fn get_dashboard(core: State<'_, SharedCore>) -> AppResult<Dashboard> {
    let core = Arc::clone(&core);
    blocking(move || core_get_dashboard(&core)).await
}

#[tauri::command]
pub async fn get_history(
    core: State<'_, SharedCore>,
    account_id: String,
) -> AppResult<Vec<HistoryPoint>> {
    let core = Arc::clone(&core);
    blocking(move || core_get_history(&core, &account_id, now_ms())).await
}

#[tauri::command]
pub async fn poll_now(core: State<'_, SharedCore>) -> AppResult<String> {
    let core = Arc::clone(&core);
    blocking(move || core_poll_now(&core)).await
}

#[tauri::command]
pub async fn add_account(
    core: State<'_, SharedCore>,
    config_dir: String,
) -> AppResult<Account> {
    let core = Arc::clone(&core);
    blocking(move || core_add_account(&core, Path::new(&config_dir), now_ms())).await
}

#[tauri::command]
pub async fn update_account(
    core: State<'_, SharedCore>,
    id: String,
    label: Option<String>,
    enabled: Option<bool>,
) -> AppResult<Account> {
    let core = Arc::clone(&core);
    blocking(move || core_update_account(&core, &id, label.as_deref(), enabled)).await
}

#[tauri::command]
pub async fn remove_account(core: State<'_, SharedCore>, id: String) -> AppResult<()> {
    let core = Arc::clone(&core);
    blocking(move || core_remove_account(&core, &id)).await
}

#[tauri::command]
pub async fn rescan_profiles(core: State<'_, SharedCore>) -> AppResult<Vec<Account>> {
    let core = Arc::clone(&core);
    blocking(move || {
        let home = crate::paths::home_dir()?;
        core_rescan_profiles(&core, &home, now_ms())
    })
    .await
}

#[tauri::command]
pub async fn get_settings(
    app: tauri::AppHandle,
    core: State<'_, SharedCore>,
) -> AppResult<UserSettings> {
    let launch_at_login = app.autolaunch().is_enabled().unwrap_or(false);
    let core = Arc::clone(&core);
    blocking(move || core_get_settings(&core, launch_at_login)).await
}

#[tauri::command]
pub async fn set_settings(
    app: tauri::AppHandle,
    core: State<'_, SharedCore>,
    settings: UserSettings,
) -> AppResult<()> {
    let want_autostart = settings.launch_at_login;
    let core_ref = Arc::clone(&core);
    let to_save = settings.clone();
    blocking(move || core_set_settings(&core_ref, &to_save)).await?;

    // Written through to the plugin's live state; never stored in the table.
    let result = if want_autostart {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    };
    if let Err(e) = result {
        warn!(error = %e, want_autostart, "could not update launch-at-login");
        return Err(AppError::Internal(format!(
            "could not update launch at login: {e}"
        )));
    }
    Ok(())
}

#[tauri::command]
pub async fn clear_halt(core: State<'_, SharedCore>) -> AppResult<()> {
    let core = Arc::clone(&core);
    blocking(move || core_clear_halt(&core)).await
}

#[tauri::command]
pub async fn open_login(core: State<'_, SharedCore>, id: String) -> AppResult<()> {
    let core = Arc::clone(&core);
    blocking(move || core_open_login(&core, &id)).await
}

#[tauri::command]
pub async fn open_log_dir(app: tauri::AppHandle, core: State<'_, SharedCore>) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = core.log_dir.clone();
    crate::paths::ensure_dir(&dir)?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| AppError::Internal(format!("could not open the log folder: {e}")))
}

#[tauri::command]
pub async fn get_snapshot_raw(
    core: State<'_, SharedCore>,
    snapshot_id: i64,
) -> AppResult<RawSnapshot> {
    let core = Arc::clone(&core);
    blocking(move || core_get_snapshot_raw(&core, snapshot_id)).await
}
```

- [ ] **Step 10: Run the full suite and the linter** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
```

Expect everything green. If clippy flags the unused `Manager` import, remove it; it is re-added in Task 21 where state is registered.

- [ ] **Step 11: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 19: add app state, coalescing triggers and the command surface

Adds the Notify-per-kind trigger set that coalesces refresh clicks and
accumulates account-changed ids, and the thirteen commands split into a
Tauri-free core that is fully unit-tested plus thin wrappers that run their
blocking work on the blocking pool. Commands hold no machine handle: gate,
busy, stalled_at and backoff_until all come from the DriverStatus snapshot
the driver publishes. set_settings publishes on the settings watch only for
interval, timeout and binary path, applies the log level directly, and
never touches the halt flag; clear_halt clears the flag and starts no poll.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 20: Scheduler driver

**Files:** Create `src-tauri/src/scheduler/driver.rs`, `src-tauri/tests/driver_loop.rs`; Modify `src-tauri/src/scheduler/mod.rs`
**Interfaces:** Consumes: `Core` / `lock_status` / `lock_binary` (Task 19), `Machine` / `SharedMachine` / `DriverStatus` / `Recorded` / `begin_cycle` / `CycleToken` / `Decision` / `Trigger` (Task 12), `run_usage` (Task 15), `Triggers` (Task 19). Produces: `pub trait EventSink`, `pub trait ProcessProbe`, `pub trait BinaryProbe`, `pub struct RealBinaryProbe`, `pub struct SysinfoProbe`, `pub fn deadline_for(last_cycle_end_ms: i64, interval_secs: u32) -> i64`, `pub fn watchdog_limit_ms(enabled_accounts: usize, timeout_secs: u32) -> u64`, `pub fn publish_status(&SharedMachine, &Mutex<DriverStatus>, i64)`, `pub trait HaltSink` + `pub fn perform_halt<S: HaltSink>(&S, i64, &str, &str) -> AppResult<()>`, `pub struct Driver` with `new(...)` and `async fn run(self)`, `pub const PRUNE_INTERVAL_MS: i64`.

The driver is the sole owner of `Machine` and the sole writer of `DriverStatus`. It calls `publish_status` after every `decide()` and every `record()` (spec §5.1), so nothing else ever needs a machine handle.

The three traits exist so the loop can be driven in tests without a Tauri runtime, a real `claude` binary, or a real process table. The production implementations live in Task 21.

**Store access rule:** every `Store` call reached from this file goes through `commands::blocking`, the shared `spawn_blocking` hop. `busy_timeout=5000` means a contended statement can park its thread for five seconds, which must never be a runtime worker (spec §6.6). That makes `settings`, `halted`, `enabled` and `decide_and_maybe_run` async, and it is why the halt sink owns its data instead of borrowing.

- [ ] **Step 1: Write the failing test for the pure helpers and the halt ordering** — create `src-tauri/src/scheduler/driver.rs` containing only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

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
```

- [ ] **Step 2: Run test to verify it fails** — first add `pub mod driver;` to `src-tauri/src/scheduler/mod.rs`, then run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib scheduler::driver::
```

Expect `cannot find function 'deadline_for' in this scope` and `cannot find trait 'HaltSink' in this scope`.

- [ ] **Step 3: Write the traits and pure helpers** — prepend to `src-tauri/src/scheduler/driver.rs`, above the test module:

```rust
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::commands::{blocking, lock_binary, lock_status, Core};
use crate::error::{AppError, AppResult};
use crate::scheduler::machine::{
    begin_cycle, lock_machine, CycleToken, Decision, DriverStatus, Machine, Recorded,
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
}

impl SysinfoProbe {
    pub fn new() -> AppResult<SysinfoProbe> {
        Ok(SysinfoProbe {
            system: Mutex::new(crate::process::new_system()?),
        })
    }
}

impl ProcessProbe for SysinfoProbe {
    fn claude_running(&self, exclude_pid: Option<u32>) -> bool {
        let mut sys = self
            .system
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        crate::process::is_claude_running(&mut sys, exclude_pid.map(sysinfo::Pid::from_u32))
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
pub fn perform_halt<S: HaltSink>(
    sink: &S,
    now_ms: i64,
    raw: &str,
    reason: &str,
) -> AppResult<()> {
    sink.persist_halt(&halt_value(now_ms))?;
    sink.log_envelope(raw, reason);
    sink.persist_outcome()?;
    sink.disable_account()?;
    Ok(())
}
```

- [ ] **Step 4: Run the helper tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib scheduler::driver::
```

Expect 10 passing tests.

- [ ] **Step 5: Write the failing test for the loop** — create `src-tauri/tests/driver_loop.rs`:

```rust
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
}

struct FixedProcess(AtomicBool);
impl ProcessProbe for FixedProcess {
    fn claude_running(&self, _exclude_pid: Option<u32>) -> bool {
        self.0.load(Ordering::SeqCst)
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
        settings_tx,
        log: None,
        app_data_dir: tmp.path().to_path_buf(),
        log_dir: tmp.path().join("logs"),
    });
    Harness {
        _tmp: tmp,
        core,
        events: Arc::new(Recorder::default()),
        process: Arc::new(FixedProcess(AtomicBool::new(running))),
        shutdown: CancellationToken::new(),
    }
}

fn add_account(h: &Harness, name: &str) -> String {
    let dir = h.core.app_data_dir.join(name);
    std::fs::create_dir_all(&dir).expect("mkdir");
    h.core
        .store
        .add_account(&dir, true, None, false, 1)
        .expect("add")
        .id
}

fn emit_ok_report() {
    let report = "Current session: 15% used \u{b7} resets Sep 16, 3:30am (America/Los_Angeles)\\n\
                  Current week (all models): 4% used \u{b7} resets Sep 21, 8am (America/Los_Angeles)";
    std::env::set_var("FAKE_CLAUDE_MODE", "emit");
    std::env::set_var(
        "FAKE_CLAUDE_STDOUT",
        format!(
            r#"{{"type":"result","local_command":"usage","num_turns":0,"total_cost_usd":0,"result":"{report}"}}"#
        ),
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_startup_cycle_polls_every_enabled_account_and_emits_one_cycle_finished() {
    emit_ok_report();
    let h = harness(false);
    let a = add_account(&h, ".claude");
    let b = add_account(&h, ".claude3");

    let driver = Driver::new(
        Arc::clone(&h.core),
        Arc::clone(&h.events) as Arc<dyn EventSink>,
        Arc::clone(&h.process) as Arc<dyn ProcessProbe>,
        Arc::new(FakeBinary(fake_claude())) as Arc<dyn BinaryProbe>,
        h.shutdown.clone(),
    );
    let handle = tokio::spawn(driver.run());

    tokio::time::sleep(Duration::from_secs(3)).await;
    h.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;

    let updated = h.events.usage_updated.lock().expect("lock").clone();
    assert!(updated.contains(&a));
    assert!(updated.contains(&b));
    assert!(h.events.cycles.load(Ordering::SeqCst) >= 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn triggers_arriving_during_a_cycle_coalesce_and_never_queue() {
    // A slow child keeps the cycle busy while clicks pile up.
    std::env::set_var("FAKE_CLAUDE_MODE", "slow");
    std::env::set_var("FAKE_CLAUDE_SLEEP_SECS", "30");
    let h = harness(false);
    add_account(&h, ".claude");
    *lock_binary(&h.core.binary) =
        Some((fake_claude().to_string_lossy().to_string(), "override"));

    let driver = Driver::new(
        Arc::clone(&h.core),
        Arc::clone(&h.events) as Arc<dyn EventSink>,
        Arc::clone(&h.process) as Arc<dyn ProcessProbe>,
        Arc::new(FakeBinary(fake_claude())) as Arc<dyn BinaryProbe>,
        h.shutdown.clone(),
    );
    let handle = tokio::spawn(driver.run());

    tokio::time::sleep(Duration::from_millis(500)).await;
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
    emit_ok_report();
    let h = harness(false);
    let a = add_account(&h, ".claude");
    let b = add_account(&h, ".claude3");
    core_update_account(&h.core, &a, None, Some(false)).expect("disable a");
    core_update_account(&h.core, &b, None, Some(false)).expect("disable b");
    let _ = h.core.triggers.take_changed();

    let driver = Driver::new(
        Arc::clone(&h.core),
        Arc::clone(&h.events) as Arc<dyn EventSink>,
        Arc::clone(&h.process) as Arc<dyn ProcessProbe>,
        Arc::new(FakeBinary(fake_claude())) as Arc<dyn BinaryProbe>,
        h.shutdown.clone(),
    );
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
    emit_ok_report();
    let h = harness(false);
    add_account(&h, ".claude");
    h.core
        .store
        .set_polling_halted("guard_tripped:1")
        .expect("halt");

    let driver = Driver::new(
        Arc::clone(&h.core),
        Arc::clone(&h.events) as Arc<dyn EventSink>,
        Arc::clone(&h.process) as Arc<dyn ProcessProbe>,
        Arc::new(FakeBinary(fake_claude())) as Arc<dyn BinaryProbe>,
        h.shutdown.clone(),
    );
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
    // A result envelope with no `type` is a shape error every time, so the
    // same account strikes out on the fifth cycle.
    std::env::set_var("FAKE_CLAUDE_MODE", "emit");
    std::env::set_var("FAKE_CLAUDE_STDOUT", r#"{"local_command":"usage"}"#);
    let h = harness(false);
    add_account(&h, ".claude");
    // A tiny gap so five cycles fit inside the test, and no backoff wait.
    let mut s = defaults();
    s.interval_secs = 10;
    h.core.store.save_settings(&s).expect("save");

    let driver = Driver::new(
        Arc::clone(&h.core),
        Arc::clone(&h.events) as Arc<dyn EventSink>,
        Arc::clone(&h.process) as Arc<dyn ProcessProbe>,
        Arc::new(FakeBinary(fake_claude())) as Arc<dyn BinaryProbe>,
        h.shutdown.clone(),
    );
    let handle = tokio::spawn(driver.run());

    // Backoff makes the timer path slow, so drive the strikes with manual
    // polls, which bypass backoff.
    for _ in 0..5 {
        let _ = core_poll_now(&h.core);
        tokio::time::sleep(Duration::from_millis(700)).await;
    }

    h.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(10), handle).await;

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
    assert_eq!(
        accounts[0].disabled_reason,
        Some(cut_core::usage::DisabledReason::GuardTripped)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guard_trip_halts_the_poller_and_abandons_the_rest_of_the_cycle() {
    std::env::set_var("FAKE_CLAUDE_MODE", "emit");
    std::env::set_var(
        "FAKE_CLAUDE_STDOUT",
        r#"{"type":"result","num_turns":1,"total_cost_usd":0.75,"result":"hello"}"#,
    );
    let h = harness(false);
    let a = add_account(&h, ".claudeA");
    let b = add_account(&h, ".claudeB");
    let c = add_account(&h, ".claudeC");

    let driver = Driver::new(
        Arc::clone(&h.core),
        Arc::clone(&h.events) as Arc<dyn EventSink>,
        Arc::clone(&h.process) as Arc<dyn ProcessProbe>,
        Arc::new(FakeBinary(fake_claude())) as Arc<dyn BinaryProbe>,
        h.shutdown.clone(),
    );
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

    let rows: i64 = h
        .core
        .store
        .with_conn(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM snapshots", [], |r| r.get(0))?))
        .expect("count");
    assert_eq!(
        rows, 1,
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
async fn the_watchdog_aborts_a_hung_cycle_and_busy_clears() {
    // A child that sleeps far past the watchdog limit, with the per-poll
    // timeout raised so the timeout path cannot rescue it first.
    std::env::set_var("FAKE_CLAUDE_MODE", "slow");
    std::env::set_var("FAKE_CLAUDE_SLEEP_SECS", "300");
    let h = harness(false);
    add_account(&h, ".claude");
    let mut s = defaults();
    s.timeout_secs = 120;
    h.core.store.save_settings(&s).expect("save");

    let driver = Driver::new(
        Arc::clone(&h.core),
        Arc::clone(&h.events) as Arc<dyn EventSink>,
        Arc::clone(&h.process) as Arc<dyn ProcessProbe>,
        Arc::new(FakeBinary(fake_claude())) as Arc<dyn BinaryProbe>,
        h.shutdown.clone(),
    );
    let handle = tokio::spawn(driver.run());

    // watchdog_limit_ms(1, 120) is 130 s, which is longer than this test
    // should run, so drive the limit down by shrinking the timeout instead.
    let mut s = defaults();
    s.timeout_secs = 5;
    core_set_settings(&h.core, &s).expect("shrink the limit");

    tokio::time::sleep(Duration::from_secs(25)).await;
    h.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(10), handle).await;

    assert!(
        !lock_status(&h.core.status).busy,
        "aborting the cycle task drops its token, and the driver republishes"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_kills_a_live_child_and_returns_promptly() {
    std::env::set_var("FAKE_CLAUDE_MODE", "slow");
    std::env::set_var("FAKE_CLAUDE_SLEEP_SECS", "300");
    let h = harness(false);
    add_account(&h, ".claude");

    let driver = Driver::new(
        Arc::clone(&h.core),
        Arc::clone(&h.events) as Arc<dyn EventSink>,
        Arc::clone(&h.process) as Arc<dyn ProcessProbe>,
        Arc::new(FakeBinary(fake_claude())) as Arc<dyn BinaryProbe>,
        h.shutdown.clone(),
    );
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
    emit_ok_report();
    let h = harness(false);
    add_account(&h, ".claude");

    let driver = Driver::new(
        Arc::clone(&h.core),
        Arc::clone(&h.events) as Arc<dyn EventSink>,
        Arc::clone(&h.process) as Arc<dyn ProcessProbe>,
        Arc::new(NoBinary) as Arc<dyn BinaryProbe>,
        h.shutdown.clone(),
    );
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
    std::env::set_var("FAKE_CLAUDE_MODE", "exit-nonzero");
    std::env::set_var("FAKE_CLAUDE_EXIT", "7");
    std::env::set_var("FAKE_CLAUDE_STDERR", "auth failed");
    let h = harness(true);
    let a = add_account(&h, ".claude");

    let driver = Driver::new(
        Arc::clone(&h.core),
        Arc::clone(&h.events) as Arc<dyn EventSink>,
        Arc::clone(&h.process) as Arc<dyn ProcessProbe>,
        Arc::new(FakeBinary(fake_claude())) as Arc<dyn BinaryProbe>,
        h.shutdown.clone(),
    );
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
```

- [ ] **Step 6: Run test to verify it fails** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --test driver_loop
```

Expect `cannot find type 'Driver' in 'cut_core::scheduler::driver'`.

- [ ] **Step 7: Write the cycle runner** — append to `src-tauri/src/scheduler/driver.rs`, above the test module:

```rust
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

        let (outcome, halt_reason) = match (&result.outcome, recorded) {
            (PollOutcome::GuardTripped(reason), _) => {
                (result.outcome.clone(), Some(reason.clone()))
            }
            (_, Recorded::Escalate) => {
                let reason = "unclassifiable envelope x5".to_string();
                (PollOutcome::GuardTripped(reason.clone()), Some(reason))
            }
            _ => (result.outcome.clone(), None),
        };

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
            let reason_for_log = reason.clone();
            if let Err(e) =
                blocking(move || perform_halt(&sink, taken_at, &envelope, &reason_for_log)).await
            {
                error!(error = %e, "could not fully record the guard trip");
            }
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
```

- [ ] **Step 8: Write the driver loop** — append to `src-tauri/src/scheduler/driver.rs`, still above the test module:

```rust
pub struct Driver {
    core: Arc<Core>,
    events: Arc<dyn EventSink>,
    process: Arc<dyn ProcessProbe>,
    binary: Arc<dyn BinaryProbe>,
    shutdown: CancellationToken,
    pid_slot: Arc<AtomicU32>,
    /// Owned here and nowhere else. Everything outside this file reads the
    /// `DriverStatus` snapshot instead (spec 5.1).
    machine: SharedMachine,
}

struct LiveCycle {
    handle: tokio::task::JoinHandle<()>,
    cancel: CancellationToken,
}

impl Driver {
    pub fn new(
        core: Arc<Core>,
        events: Arc<dyn EventSink>,
        process: Arc<dyn ProcessProbe>,
        binary: Arc<dyn BinaryProbe>,
        shutdown: CancellationToken,
    ) -> Driver {
        Driver {
            core,
            events,
            process,
            binary,
            shutdown,
            pid_slot: Arc::new(AtomicU32::new(0)),
            machine: Arc::new(Mutex::new(Machine::new())),
        }
    }

    fn current_pid(&self) -> Option<u32> {
        match self.pid_slot.load(Ordering::SeqCst) {
            0 => None,
            p => Some(p),
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
    ) -> LiveCycle {
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
        let handle = tokio::spawn(run_cycle(inputs, token));
        LiveCycle { handle, cancel }
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

    /// Runs one decision and, on `Run`, starts the cycle. Async because the
    /// enabled list and the halt flag both come from the store.
    async fn decide_and_maybe_run(
        &self,
        trigger: Trigger,
        claude_running: Option<bool>,
        settings: &UserSettings,
    ) -> Option<LiveCycle> {
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
                    | crate::scheduler::machine::SkipReason::Busy => {
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
                    info!(gate = gate.as_str(), "gate changed");
                    self.events.gate_changed(gate.as_str());
                }
                let binary = binary_path?;
                Some(self.start_cycle(accounts, reason, binary, settings))
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

        // Startup: decide, run, then enter the loop.
        if let Some(cycle) = self
            .decide_and_maybe_run(Trigger::Startup, None, &settings)
            .await
        {
            live = Some(cycle);
        }

        let mut last_cycle_end = chrono::Utc::now().timestamp_millis();
        let mut watchdog = tokio::time::interval(WATCHDOG_TICK);
        watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

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

            // Reap a finished cycle and anchor the next deadline on its end.
            if let Some(cycle) = live.as_ref() {
                if cycle.handle.is_finished() {
                    live = None;
                    last_cycle_end = chrono::Utc::now().timestamp_millis();
                    lock_status(&self.core.status).stalled_at = None;
                    // The token has dropped, so busy is false again.
                    self.publish();
                }
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
                    // Busy is checked before spending a process check, so the
                    // app's own child can never latch the gate.
                    if lock_machine(&self.machine).is_busy() {
                        debug!("timer skipped: a cycle is already running");
                        last_cycle_end = chrono::Utc::now().timestamp_millis();
                        continue;
                    }
                    let running = self.process.claude_running(self.current_pid());
                    if let Some(cycle) = self
                        .decide_and_maybe_run(Trigger::Timer, Some(running), &settings)
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
                    if let Some(cycle) = self
                        .decide_and_maybe_run(Trigger::Manual, None, &settings)
                        .await
                    {
                        live = Some(cycle);
                    }
                }
                _ = self.core.triggers.notified_startup() => {
                    if let Some(cycle) = self
                        .decide_and_maybe_run(Trigger::Startup, None, &settings)
                        .await
                    {
                        live = Some(cycle);
                    }
                }
                _ = self.core.triggers.notified_changed() => {
                    let ids = self.core.triggers.take_changed();
                    if !ids.is_empty() {
                        if let Some(cycle) = self
                            .decide_and_maybe_run(Trigger::AccountChanged(ids), None, &settings)
                            .await
                        {
                            live = Some(cycle);
                        }
                    }
                }
                _ = watchdog.tick(), if cycle_running => {
                    let age = lock_machine(&self.machine)
                        .cycle_age(chrono::Utc::now().timestamp_millis());
                    if let (Some(age), Some(cycle)) = (age, live.as_ref()) {
                        let limit = watchdog_limit_ms(
                            self.enabled().await.len(),
                            settings.timeout_secs,
                        );
                        if age.as_millis() as u64 > limit {
                            let at = chrono::Utc::now().timestamp_millis();
                            error!(
                                cycle_age_ms = age.as_millis() as u64,
                                limit_ms = limit,
                                "watchdog: aborting a stalled cycle task"
                            );
                            cycle.cancel.cancel();
                            cycle.handle.abort();
                            lock_status(&self.core.status).stalled_at = Some(at);
                            self.events.poller_stalled(at, age.as_millis() as u64);
                            live = None;
                            last_cycle_end = at;
                            // Aborting dropped the token, so busy is clear.
                            self.publish();
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
```

- [ ] **Step 9: Run the driver tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --test driver_loop -- --test-threads=1
```

The single test thread matters: these tests configure the fake binary through process-wide environment variables. Expect 10 passing tests. They take roughly two minutes in total.

- [ ] **Step 10: Run the full suite and the linter** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test -- --test-threads=1
cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 11: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 20: add the scheduler driver loop

The loop never polls inline: a Run decision spawns a cycle task owning the
RAII token and a child cancellation token, so any trigger arriving during a
cycle is decided immediately and gets skipped as busy. The driver is the
sole owner of the state machine and the sole writer of DriverStatus, which
it publishes after every decide and every record so nothing else needs a
machine handle. Adds the gap-based deadline anchored on the last cycle end,
the settings watch that moves the deadline and resets backoff without
restarting the clock, the watchdog that aborts a stalled cycle, daily
pruning, and the guard-trip path that persists the halt flag before
anything else and abandons the rest of the cycle. Event emission, the
process gate and binary discovery sit behind traits so the loop is testable
without a Tauri runtime.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 21: Tray icons and the Tauri application wiring

**Files:** Modify `src-tauri/src/tray.rs`, `src-tauri/src/lib.rs`
**Interfaces:** Consumes: `tray_state` / `Level` (Task 16), `Core` / every command (Task 19), `DriverStatus` (Task 12), `Driver` / `EventSink` / `SysinfoProbe` / `RealBinaryProbe` (Task 20), `init_logging` (Task 17), `Store` (Tasks 8–11), `enumerate_profiles` (Task 7), `paths` (Task 6). Produces: `pub const MENU_OPEN/MENU_REFRESH/MENU_LOGS/MENU_QUIT: &str`, `pub fn level_rgb(Level) -> [u8; 3]`, `pub fn icon_rgba(Level, u32) -> Vec<u8>`, `pub fn should_hide_on_close(bool) -> bool`, `pub fn apply_tray(&tauri::AppHandle, &Core)`, `pub struct TauriEvents`, `pub fn run()`.

- [ ] **Step 1: Write the failing test for the icon and close behaviour** — append these tests inside the existing `mod tests` block in `src-tauri/src/tray.rs`:

```rust
    #[test]
    fn menu_ids_are_stable_snake_case_strings() {
        assert_eq!(MENU_OPEN, "open");
        assert_eq!(MENU_REFRESH, "refresh_now");
        assert_eq!(MENU_LOGS, "open_log_folder");
        assert_eq!(MENU_QUIT, "quit");
    }

    #[test]
    fn every_level_has_a_distinct_colour() {
        let colours = [
            level_rgb(Level::Halted),
            level_rgb(Level::Grey),
            level_rgb(Level::Green),
            level_rgb(Level::Amber),
            level_rgb(Level::Red),
        ];
        for (i, a) in colours.iter().enumerate() {
            for (j, b) in colours.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "levels {i} and {j} must look different");
                }
            }
        }
    }

    #[test]
    fn the_icon_is_a_square_rgba_buffer() {
        let size = 32u32;
        let buf = icon_rgba(Level::Green, size);
        assert_eq!(buf.len(), (size * size * 4) as usize);
    }

    #[test]
    fn the_icon_centre_carries_the_level_colour_at_full_opacity() {
        let size = 32u32;
        let buf = icon_rgba(Level::Red, size);
        let centre = ((size / 2 * size) + size / 2) as usize * 4;
        assert_eq!(&buf[centre..centre + 3], &level_rgb(Level::Red));
        assert_eq!(buf[centre + 3], 255);
    }

    #[test]
    fn the_icon_corners_are_transparent() {
        let size = 32u32;
        let buf = icon_rgba(Level::Green, size);
        assert_eq!(buf[3], 0, "top-left pixel must be transparent");
        let last = ((size * size - 1) * 4 + 3) as usize;
        assert_eq!(buf[last], 0, "bottom-right pixel must be transparent");
    }

    #[test]
    fn the_halted_icon_carries_a_badge_the_others_do_not() {
        let size = 32u32;
        let halted = icon_rgba(Level::Halted, size);
        let green = icon_rgba(Level::Green, size);
        // The badge sits in the top-right quadrant.
        let badge = ((size / 5 * size) + (size * 4 / 5)) as usize * 4;
        assert_eq!(halted[badge + 3], 255, "the halted badge must be opaque");
        assert_eq!(green[badge + 3], 0, "other levels must have no badge");
    }

    #[test]
    fn close_to_tray_decides_whether_a_window_close_hides_or_quits() {
        assert!(should_hide_on_close(true));
        assert!(!should_hide_on_close(false));
    }
```

- [ ] **Step 2: Run test to verify it fails** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib tray::
```

Expect `cannot find value 'MENU_OPEN' in this scope` and `cannot find function 'icon_rgba' in this scope`.

- [ ] **Step 3: Write the icon and menu constants** — append to the implementation section of `src-tauri/src/tray.rs`, above the test module:

```rust
pub const MENU_OPEN: &str = "open";
pub const MENU_REFRESH: &str = "refresh_now";
pub const MENU_LOGS: &str = "open_log_folder";
pub const MENU_QUIT: &str = "quit";

/// Icons are drawn rather than shipped as assets, so the five states stay in
/// sync with `Level` and there is nothing to keep in a bundle.
pub fn level_rgb(level: Level) -> [u8; 3] {
    match level {
        Level::Halted => [0x8B, 0x1A, 0x1A],
        Level::Grey => [0x8A, 0x8A, 0x8A],
        Level::Green => [0x2E, 0xA0, 0x43],
        Level::Amber => [0xD2, 0x96, 0x22],
        Level::Red => [0xD7, 0x33, 0x33],
    }
}

/// A filled circle in the level colour, plus a small opaque badge in the
/// top-right quadrant for `Halted` so the halted state is distinguishable at
/// tray size even in monochrome.
pub fn icon_rgba(level: Level, size: u32) -> Vec<u8> {
    let rgb = level_rgb(level);
    let mut buf = vec![0u8; (size * size * 4) as usize];
    let centre = size as f32 / 2.0;
    let radius = centre - 1.0;
    let badge_centre = (size as f32 * 0.8, size as f32 * 0.2);
    let badge_radius = size as f32 * 0.22;

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let dx = x as f32 + 0.5 - centre;
            let dy = y as f32 + 0.5 - centre;
            let inside = dx * dx + dy * dy <= radius * radius;

            let bdx = x as f32 + 0.5 - badge_centre.0;
            let bdy = y as f32 + 0.5 - badge_centre.1;
            let in_badge = level == Level::Halted
                && bdx * bdx + bdy * bdy <= badge_radius * badge_radius;

            if in_badge {
                buf[idx] = 0xFF;
                buf[idx + 1] = 0xCC;
                buf[idx + 2] = 0x00;
                buf[idx + 3] = 255;
            } else if inside {
                buf[idx] = rgb[0];
                buf[idx + 1] = rgb[1];
                buf[idx + 2] = rgb[2];
                buf[idx + 3] = 255;
            }
        }
    }
    buf
}

/// Window close hides to the tray unless the user turned that off.
pub fn should_hide_on_close(close_to_tray: bool) -> bool {
    close_to_tray
}
```

- [ ] **Step 4: Run the icon tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test --lib tray::
cargo clippy --all-targets -- -D warnings
```

Expect 19 passing tests, no warnings.

- [ ] **Step 5: Write `apply_tray`** — append to `src-tauri/src/tray.rs`, still above the test module:

```rust
use tauri::image::Image;
use tauri::Manager;
use tracing::warn;

use crate::commands::Core;

const TRAY_ICON_SIZE: u32 = 32;

/// Recomputes the level and tooltip from the store and pushes them onto the
/// tray icon. Called after each account's poll and on account/settings change.
pub fn apply_tray(app: &tauri::AppHandle, core: &Core) {
    let accounts = match core.store.list_accounts() {
        Ok(a) => a,
        Err(e) => {
            warn!(error = %e, "could not read accounts for the tray");
            return;
        }
    };
    let latest = core.store.latest_per_account().unwrap_or_default();
    let halted = core
        .store
        .polling_halted()
        .unwrap_or_default()
        .is_some();

    let rows: Vec<(Account, Option<SnapshotDto>)> = accounts
        .into_iter()
        .map(|a| {
            let snap = latest.get(&a.id).cloned();
            (a, snap)
        })
        .collect();

    let (level, tooltip) = tray_state(&rows, halted);

    let tray = match app.tray_by_id("main") {
        Some(t) => t,
        None => {
            warn!("tray icon 'main' not found");
            return;
        }
    };

    let rgba = icon_rgba(level, TRAY_ICON_SIZE);
    let image = Image::new_owned(rgba, TRAY_ICON_SIZE, TRAY_ICON_SIZE);
    if let Err(e) = tray.set_icon(Some(image)) {
        warn!(error = %e, "could not set the tray icon");
    }
    if let Err(e) = tray.set_tooltip(Some(&tooltip)) {
        warn!(error = %e, "could not set the tray tooltip");
    }
}
```

- [ ] **Step 6: Write the Tauri event sink** — append to `src-tauri/src/tray.rs`, still above the test module:

```rust
use serde::Serialize;
use std::sync::Arc;
use tauri::Emitter;

use crate::scheduler::driver::EventSink;

#[derive(Serialize, Clone)]
struct AccountEvent<'a> {
    account_id: &'a str,
}

#[derive(Serialize, Clone)]
struct GateEvent<'a> {
    gate: &'a str,
}

#[derive(Serialize, Clone)]
struct StalledEvent {
    at: i64,
    cycle_age_ms: u64,
}

/// Events are refetch triggers only: the frontend ignores the payloads and
/// re-reads state through commands. The payloads exist for logs and tests.
pub struct TauriEvents {
    app: tauri::AppHandle,
    core: Arc<Core>,
}

impl TauriEvents {
    pub fn new(app: tauri::AppHandle, core: Arc<Core>) -> TauriEvents {
        TauriEvents { app, core }
    }
}

impl EventSink for TauriEvents {
    fn usage_updated(&self, account_id: &str) {
        let _ = self.app.emit("usage:updated", AccountEvent { account_id });
    }
    fn cycle_finished(&self) {
        let _ = self.app.emit("cycle:finished", ());
    }
    fn gate_changed(&self, gate: &str) {
        let _ = self.app.emit("gate:changed", GateEvent { gate });
    }
    fn poller_stalled(&self, at: i64, cycle_age_ms: u64) {
        let _ = self
            .app
            .emit("poller:stalled", StalledEvent { at, cycle_age_ms });
    }
    fn refresh_tray(&self) {
        apply_tray(&self.app, &self.core);
    }
}
```

- [ ] **Step 7: Write the application builder** — replace `src-tauri/src/lib.rs` entirely:

```rust
pub mod commands;
pub mod discovery;
pub mod error;
pub mod logging;
pub mod login;
pub mod paths;
pub mod process;
pub mod scheduler;
pub mod store;
pub mod tray;
pub mod usage;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, RunEvent, WindowEvent};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::commands::{Core, SharedCore};
use crate::scheduler::driver::{
    BinaryProbe, Driver, EventSink, ProcessProbe, RealBinaryProbe, SysinfoProbe,
};
use crate::scheduler::machine::DriverStatus;
use crate::scheduler::triggers::Triggers;
use crate::store::Store;
use crate::tray::{
    apply_tray, should_hide_on_close, TauriEvents, MENU_LOGS, MENU_OPEN, MENU_QUIT, MENU_REFRESH,
};

/// Set once the shutdown sequence has finished, so the second
/// `ExitRequested` is allowed through.
static EXIT_APPROVED: AtomicBool = AtomicBool::new(false);

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Entry point called by `main.rs`.
pub fn run() {
    let shutdown = CancellationToken::new();
    let shutdown_for_event = shutdown.clone();

    let result = tauri::Builder::default()
        // Single instance must be registered first so a second launch is
        // rejected before any other plugin initialises.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_dashboard,
            commands::get_history,
            commands::poll_now,
            commands::add_account,
            commands::update_account,
            commands::remove_account,
            commands::rescan_profiles,
            commands::get_settings,
            commands::set_settings,
            commands::clear_halt,
            commands::open_login,
            commands::open_log_dir,
            commands::get_snapshot_raw,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            let app_data_dir = handle.path().app_data_dir()?;
            let log_dir = handle.path().app_log_dir()?;
            paths::ensure_dir(&app_data_dir)?;

            let store = Arc::new(Store::open(&paths::db_path(&app_data_dir))?);
            let stored = store.stored_settings()?;

            // Logging first, so everything below is captured.
            let log = match logging::init_logging(&log_dir, &stored.log_level) {
                Ok(h) => Some(Arc::new(h)),
                Err(e) => {
                    eprintln!("logging unavailable: {e}");
                    None
                }
            };
            info!(
                app_data_dir = %app_data_dir.display(),
                log_dir = %log_dir.display(),
                interval_secs = stored.interval_secs,
                "starting"
            );

            // Seed accounts on first start (spec 6.1).
            let home = paths::home_dir()?;
            let candidates = discovery::enumerate_profiles(&home);
            let default_dir = paths::default_config_dir(
                std::env::var("CLAUDE_CONFIG_DIR").ok().as_deref(),
                &home,
            );
            let seeded = store.seed_accounts_if_empty(
                &candidates,
                &default_dir,
                chrono::Utc::now().timestamp_millis(),
            )?;
            info!(seeded, discovered = candidates.len(), "accounts loaded");

            // D10: prune once at startup; the driver repeats it every 24 h.
            if let Err(e) = store.prune(chrono::Utc::now().timestamp_millis()) {
                error!(error = %e, "startup prune failed");
            }

            // The login script directory is emptied at startup (spec 6.8).
            if let Err(e) = paths::empty_dir(&paths::login_script_dir(&app_data_dir)) {
                error!(error = %e, "could not clear the login script directory");
            }

            let (settings_tx, _settings_rx) = tokio::sync::watch::channel(stored.clone());
            let core: SharedCore = Arc::new(Core {
                store,
                triggers: Arc::new(Triggers::new()),
                // The driver owns the state machine and is the only writer of
                // this snapshot (spec 5.1); everything else only reads it.
                status: Arc::new(Mutex::new(DriverStatus::default())),
                binary: Arc::new(Mutex::new(None)),
                settings_tx,
                log,
                app_data_dir,
                log_dir,
            });
            app.manage(Arc::clone(&core));

            // Tray.
            let open = MenuItem::with_id(app, MENU_OPEN, "Open", true, None::<&str>)?;
            let refresh =
                MenuItem::with_id(app, MENU_REFRESH, "Refresh now", true, None::<&str>)?;
            let logs =
                MenuItem::with_id(app, MENU_LOGS, "Open log folder", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &refresh, &logs, &quit])?;

            let menu_core = Arc::clone(&core);
            TrayIconBuilder::with_id("main")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("Claude Usage Tracker")
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    MENU_OPEN => show_main_window(app),
                    MENU_REFRESH => {
                        match commands::core_poll_now(&menu_core) {
                            Ok(s) => info!(result = %s, "tray refresh"),
                            Err(e) => error!(error = %e, "tray refresh failed"),
                        };
                    }
                    MENU_LOGS => {
                        use tauri_plugin_opener::OpenerExt;
                        let dir = menu_core.log_dir.to_string_lossy().to_string();
                        if let Err(e) = app.opener().open_path(dir, None::<&str>) {
                            error!(error = %e, "could not open the log folder");
                        }
                    }
                    MENU_QUIT => app.exit(0),
                    other => error!(id = other, "unhandled tray menu id"),
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            apply_tray(&handle, &core);

            // Scheduler.
            let events: Arc<dyn EventSink> =
                Arc::new(TauriEvents::new(handle.clone(), Arc::clone(&core)));
            let process: Arc<dyn ProcessProbe> = Arc::new(SysinfoProbe::new()?);
            let binary: Arc<dyn BinaryProbe> = Arc::new(RealBinaryProbe);
            let driver = Driver::new(
                Arc::clone(&core),
                events,
                process,
                binary,
                shutdown.clone(),
            );
            tauri::async_runtime::spawn(driver.run());

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let close_to_tray = app
                    .try_state::<SharedCore>()
                    .and_then(|c| c.store.stored_settings().ok())
                    .map(|s| s.close_to_tray)
                    .unwrap_or(true);
                if should_hide_on_close(close_to_tray) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!());

    let app = match result {
        Ok(a) => a,
        Err(e) => {
            eprintln!("fatal: could not build the application: {e}");
            std::process::exit(1);
        }
    };

    app.run(move |app_handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            if EXIT_APPROVED.load(Ordering::SeqCst) {
                return;
            }
            api.prevent_exit();
            info!("exit requested; shutting the poller down");
            shutdown_for_event.cancel();

            let handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                // `app.exit()` ends in `process::exit`, which skips
                // destructors, so the driver's own shutdown path must have
                // killed and waited on any child before we get here.
                tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
                EXIT_APPROVED.store(true, Ordering::SeqCst);
                handle.exit(0);
            });
        }
    });
}
```

- [ ] **Step 8: Run the full suite and the linter** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test -- --test-threads=1
cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 9: Build the release bundle to prove the wiring compiles end to end** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
npm run build
cd src-tauri
cargo build --release
```

Expect both to finish clean. Confirm the test helper is not in the bundle:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
ls target/release/bundle 2>/dev/null || echo "no bundle dir yet (cargo build only builds binaries)"
```

- [ ] **Step 10: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 21: wire the Tauri application, tray and shutdown hook

Registers single-instance first, then autostart and opener, opens the
store, initialises logging, seeds accounts on first start, prunes, clears
the login script directory, builds the tray with drawn per-level icons and
the four menu items, starts the scheduler, hides to the tray on window
close when configured, and cancels the poller inside ExitRequested before
allowing the second exit through.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 22: Frontend types and pure helpers

**Files:** Create `vitest.config.ts`, `src/lib/types.ts`, `src/lib/format.ts`, `src/lib/format.test.ts`, `src/lib/sparkline.ts`, `src/lib/sparkline.test.ts`, `src/lib/pill.ts`, `src/lib/pill.test.ts`, `src/lib/banner.ts`, `src/lib/banner.test.ts`
**Interfaces:** Consumes: the DTO shapes produced by Tasks 3, 11 and 19. Produces: `formatCountdown(resetsAt: number | null, now: number): string`, `formatAgo(takenAt: number | null, now: number): string`, `buildSparklinePaths(points: HistoryPoint[], width: number, height: number): string[]`, `statusPill(row: AccountRow, now: number): Pill`, `bannerFor(dashboard: Dashboard): Banner | null`, plus every DTO interface in `types.ts`.

No `any` anywhere. Every helper is a pure function of its arguments and an injected `now`.

- [ ] **Step 1: Write the Vitest config** — create `vitest.config.ts` at the repo root:

```ts
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
```

- [ ] **Step 2: Write the shared types** — create `src/lib/types.ts`:

```ts
export type Outcome =
  | "ok"
  | "no_usage_data"
  | "parse_error"
  | "spawn_error"
  | "timeout"
  | "guard_tripped";

export type DisabledReason = "user" | "guard_tripped";
export type BinarySource = "override" | "local_bin" | "path";
export type Gate = "idle" | "active";

export interface Win {
  pct: number;
  resets_at: number | null;
}

export interface ModelWindow {
  label: string;
  pct: number;
  resets_at: number | null;
}

export interface SnapshotDto {
  id: number;
  account_id: string;
  taken_at: number;
  outcome: Outcome;
  session: Win | null;
  week_all: Win | null;
  week_models: ModelWindow[];
  error: string | null;
  duration_ms: number;
}

export interface Account {
  id: string;
  label: string;
  config_dir: string;
  enabled: boolean;
  disabled_reason: DisabledReason | null;
  is_default: boolean;
  created_at: number;
}

export interface AccountRow {
  account: Account;
  latest: SnapshotDto | null;
  backoff_until: number | null;
}

export interface BinaryInfo {
  path: string | null;
  source: BinarySource | null;
}

export interface Dashboard {
  accounts: AccountRow[];
  gate: Gate;
  busy: boolean;
  halted: string | null;
  stalled_at: number | null;
  binary: BinaryInfo;
  interval_secs: number;
}

export interface HistoryPoint {
  t: number;
  pct: number;
}

export interface UserSettings {
  interval_secs: number;
  timeout_secs: number;
  claude_binary: string;
  close_to_tray: boolean;
  launch_at_login: boolean;
  log_level: "info" | "debug";
}

export interface RawSnapshot {
  raw: string | null;
  error: string | null;
}

export interface AppErrorShape {
  code: string;
  message: string;
}

/** Narrowing guard so a caught `unknown` never needs an `any` cast. */
export function isAppError(e: unknown): e is AppErrorShape {
  return (
    typeof e === "object" &&
    e !== null &&
    typeof (e as { code?: unknown }).code === "string" &&
    typeof (e as { message?: unknown }).message === "string"
  );
}
```

- [ ] **Step 3: Write the failing test for the formatters** — create `src/lib/format.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { formatAgo, formatCountdown } from "./format";

const NOW = 1_700_000_000_000;
const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

describe("formatCountdown", () => {
  it("renders an em dash when there is no reset instant", () => {
    expect(formatCountdown(null, NOW)).toBe("—");
  });

  it("clamps a past reset to 'resets now'", () => {
    expect(formatCountdown(NOW - 1, NOW)).toBe("resets now");
    expect(formatCountdown(NOW, NOW)).toBe("resets now");
    expect(formatCountdown(NOW - DAY, NOW)).toBe("resets now");
  });

  it("renders sub-minute gaps as less than a minute", () => {
    expect(formatCountdown(NOW + 1 * SECOND, NOW)).toBe("resets in <1m");
    expect(formatCountdown(NOW + 59 * SECOND, NOW)).toBe("resets in <1m");
  });

  it("renders minutes under an hour", () => {
    expect(formatCountdown(NOW + 1 * MINUTE, NOW)).toBe("resets in 1m");
    expect(formatCountdown(NOW + 42 * MINUTE, NOW)).toBe("resets in 42m");
    expect(formatCountdown(NOW + 59 * MINUTE + 59 * SECOND, NOW)).toBe(
      "resets in 59m",
    );
  });

  it("renders hours and minutes under a day", () => {
    expect(formatCountdown(NOW + 3 * HOUR + 12 * MINUTE, NOW)).toBe(
      "resets in 3h 12m",
    );
    expect(formatCountdown(NOW + 1 * HOUR, NOW)).toBe("resets in 1h 0m");
    expect(formatCountdown(NOW + 23 * HOUR + 59 * MINUTE, NOW)).toBe(
      "resets in 23h 59m",
    );
  });

  it("renders days and hours beyond a day", () => {
    expect(formatCountdown(NOW + 2 * DAY + 4 * HOUR, NOW)).toBe(
      "resets in 2d 4h",
    );
    expect(formatCountdown(NOW + 6 * DAY + 23 * HOUR, NOW)).toBe(
      "resets in 6d 23h",
    );
  });
});

describe("formatAgo", () => {
  it("says never when nothing has been recorded", () => {
    expect(formatAgo(null, NOW)).toBe("never");
  });

  it("treats a future timestamp as just now", () => {
    expect(formatAgo(NOW + 5 * SECOND, NOW)).toBe("just now");
  });

  it("renders seconds", () => {
    expect(formatAgo(NOW, NOW)).toBe("0 s ago");
    expect(formatAgo(NOW - 42 * SECOND, NOW)).toBe("42 s ago");
    expect(formatAgo(NOW - 59 * SECOND, NOW)).toBe("59 s ago");
  });

  it("renders minutes", () => {
    expect(formatAgo(NOW - 1 * MINUTE, NOW)).toBe("1 min ago");
    expect(formatAgo(NOW - 59 * MINUTE, NOW)).toBe("59 min ago");
  });

  it("renders hours", () => {
    expect(formatAgo(NOW - 1 * HOUR, NOW)).toBe("1 h ago");
    expect(formatAgo(NOW - 23 * HOUR, NOW)).toBe("23 h ago");
  });

  it("renders days", () => {
    expect(formatAgo(NOW - 1 * DAY, NOW)).toBe("1 d ago");
    expect(formatAgo(NOW - 9 * DAY, NOW)).toBe("9 d ago");
  });
});
```

- [ ] **Step 4: Run the test to verify it fails** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
npm test
```

Expect `Failed to resolve import "./format"`.

- [ ] **Step 5: Write the formatters** — create `src/lib/format.ts`:

```ts
const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/**
 * "resets in 3h 12m", clamped at "resets now" when the instant has passed,
 * and an em dash when the CLI omitted the reset clause.
 */
export function formatCountdown(resetsAt: number | null, now: number): string {
  if (resetsAt === null) {
    return "—";
  }
  const remaining = resetsAt - now;
  if (remaining <= 0) {
    return "resets now";
  }
  if (remaining < MINUTE) {
    return "resets in <1m";
  }
  if (remaining < HOUR) {
    return `resets in ${Math.floor(remaining / MINUTE)}m`;
  }
  if (remaining < DAY) {
    const hours = Math.floor(remaining / HOUR);
    const minutes = Math.floor((remaining % HOUR) / MINUTE);
    return `resets in ${hours}h ${minutes}m`;
  }
  const days = Math.floor(remaining / DAY);
  const hours = Math.floor((remaining % DAY) / HOUR);
  return `resets in ${days}d ${hours}h`;
}

/** "42 s ago", ticking live from a 1 s interval in the caller. */
export function formatAgo(takenAt: number | null, now: number): string {
  if (takenAt === null) {
    return "never";
  }
  const elapsed = now - takenAt;
  if (elapsed < 0) {
    return "just now";
  }
  if (elapsed < MINUTE) {
    return `${Math.floor(elapsed / SECOND)} s ago`;
  }
  if (elapsed < HOUR) {
    return `${Math.floor(elapsed / MINUTE)} min ago`;
  }
  if (elapsed < DAY) {
    return `${Math.floor(elapsed / HOUR)} h ago`;
  }
  return `${Math.floor(elapsed / DAY)} d ago`;
}
```

- [ ] **Step 6: Run the formatter tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
npm test
```

Expect 12 passing tests.

- [ ] **Step 7: Write the failing test for the sparkline** — create `src/lib/sparkline.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { buildSparklinePaths } from "./sparkline";
import type { HistoryPoint } from "./types";

const HOUR = 3_600_000;
const BASE = 100 * HOUR;

function pts(spec: Array<[number, number]>): HistoryPoint[] {
  return spec.map(([hours, pct]) => ({ t: BASE + hours * HOUR, pct }));
}

describe("buildSparklinePaths", () => {
  it("returns nothing for an empty series", () => {
    expect(buildSparklinePaths([], 100, 20)).toEqual([]);
  });

  it("returns one sub-path for contiguous hours", () => {
    const paths = buildSparklinePaths(pts([[0, 10], [1, 20], [2, 30]]), 100, 20);
    expect(paths).toHaveLength(1);
    expect(paths[0].startsWith("M ")).toBe(true);
    expect(paths[0].split("L")).toHaveLength(3);
  });

  it("breaks the line at a gap instead of drawing through it", () => {
    // Hours 0 and 1, then a three-hour hole, then hours 5 and 6.
    const paths = buildSparklinePaths(
      pts([[0, 10], [1, 20], [5, 30], [6, 40]]),
      100,
      20,
    );
    expect(paths).toHaveLength(2);
  });

  it("never emits a zero for a missing hour", () => {
    const paths = buildSparklinePaths(pts([[0, 50], [4, 50]]), 100, 20);
    expect(paths.join(" ")).not.toContain("20 ");
    expect(paths).toHaveLength(2);
  });

  it("maps a higher percentage to a smaller y so the line rises", () => {
    const [path] = buildSparklinePaths(pts([[0, 0], [1, 100]]), 100, 20);
    const ys = [...path.matchAll(/-?\d+(?:\.\d+)?\s+(-?\d+(?:\.\d+)?)/g)].map(
      (m) => Number(m[1]),
    );
    expect(ys[0]).toBeGreaterThan(ys[1]);
  });

  it("keeps every point inside the box", () => {
    const [path] = buildSparklinePaths(
      pts([[0, 0], [1, 50], [2, 100]]),
      120,
      24,
    );
    const numbers = [...path.matchAll(/(-?\d+(?:\.\d+)?)/g)].map((m) =>
      Number(m[1]),
    );
    for (let i = 0; i < numbers.length; i += 2) {
      expect(numbers[i]).toBeGreaterThanOrEqual(0);
      expect(numbers[i]).toBeLessThanOrEqual(120);
      expect(numbers[i + 1]).toBeGreaterThanOrEqual(0);
      expect(numbers[i + 1]).toBeLessThanOrEqual(24);
    }
  });

  it("renders a single point as its own degenerate sub-path", () => {
    const paths = buildSparklinePaths(pts([[3, 42]]), 100, 20);
    expect(paths).toHaveLength(1);
    expect(paths[0]).toMatch(/^M [\d.]+ [\d.]+ L [\d.]+ [\d.]+$/);
  });

  it("puts a lone point at the right edge of the box", () => {
    const [path] = buildSparklinePaths(pts([[3, 42]]), 100, 20);
    expect(path.startsWith("M 100 ")).toBe(true);
  });

  it("sorts unordered input before drawing", () => {
    const unordered = pts([[2, 30], [0, 10], [1, 20]]);
    const ordered = pts([[0, 10], [1, 20], [2, 30]]);
    expect(buildSparklinePaths(unordered, 100, 20)).toEqual(
      buildSparklinePaths(ordered, 100, 20),
    );
  });
});
```

- [ ] **Step 8: Run the test to verify it fails** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
npm test
```

Expect `Failed to resolve import "./sparkline"`.

- [ ] **Step 9: Write the sparkline path builder** — create `src/lib/sparkline.ts`:

```ts
import type { HistoryPoint } from "./types";

const HOUR = 3_600_000;

function round(n: number): number {
  return Math.round(n * 100) / 100;
}

/**
 * Hand-rolled SVG path data for the hourly week-all series.
 *
 * Missing hours are breaks, never zeros: the process gate guarantees
 * overnight gaps, so any hour more than one bucket after its predecessor
 * starts a new sub-path. The caller renders one <path> per string.
 */
export function buildSparklinePaths(
  points: HistoryPoint[],
  width: number,
  height: number,
): string[] {
  if (points.length === 0) {
    return [];
  }

  const sorted = [...points].sort((a, b) => a.t - b.t);
  const first = sorted[0].t;
  const last = sorted[sorted.length - 1].t;
  const span = last - first;

  const x = (t: number): number =>
    span === 0 ? width : round(((t - first) / span) * width);
  const y = (pct: number): number =>
    round(height - (Math.min(Math.max(pct, 0), 100) / 100) * height);

  const segments: HistoryPoint[][] = [];
  let current: HistoryPoint[] = [sorted[0]];

  for (let i = 1; i < sorted.length; i += 1) {
    const gap = sorted[i].t - sorted[i - 1].t;
    if (gap > HOUR) {
      segments.push(current);
      current = [sorted[i]];
    } else {
      current.push(sorted[i]);
    }
  }
  segments.push(current);

  return segments.map((segment) => {
    const head = segment[0];
    const start = `M ${x(head.t)} ${y(head.pct)}`;
    if (segment.length === 1) {
      // A lone reading still has to be visible, so draw a zero-length line.
      return `${start} L ${x(head.t)} ${y(head.pct)}`;
    }
    const rest = segment
      .slice(1)
      .map((p) => `L ${x(p.t)} ${y(p.pct)}`)
      .join(" ");
    return `${start} ${rest}`;
  });
}
```

- [ ] **Step 10: Run the sparkline tests to verify they pass** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
npm test
```

Expect 21 passing tests in total.

- [ ] **Step 11: Write the failing test for the status pill** — create `src/lib/pill.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { statusPill } from "./pill";
import type { Account, AccountRow, Outcome, SnapshotDto } from "./types";

const NOW = 1_700_000_000_000;

function account(over: Partial<Account> = {}): Account {
  return {
    id: "a1",
    label: "claude3",
    config_dir: "/home/josh/.claude3",
    enabled: true,
    disabled_reason: null,
    is_default: false,
    created_at: 0,
    ...over,
  };
}

function snapshot(outcome: Outcome): SnapshotDto {
  return {
    id: 1,
    account_id: "a1",
    taken_at: NOW - 1000,
    outcome,
    session: null,
    week_all: null,
    week_models: [],
    error: outcome === "ok" ? null : "boom",
    duration_ms: 10,
  };
}

function row(over: Partial<AccountRow> = {}): AccountRow {
  return {
    account: account(),
    latest: snapshot("ok"),
    backoff_until: null,
    ...over,
  };
}

describe("statusPill", () => {
  it("shows disabled ahead of everything else", () => {
    const pill = statusPill(
      row({
        account: account({ enabled: false, disabled_reason: "user" }),
        backoff_until: NOW + 240_000,
        latest: snapshot("spawn_error"),
      }),
      NOW,
    );
    expect(pill.kind).toBe("disabled");
    expect(pill.label).toBe("disabled");
    expect(pill.tooltip).toBe("disabled by you");
  });

  it("explains a guard-tripped disable in the tooltip", () => {
    const pill = statusPill(
      row({
        account: account({ enabled: false, disabled_reason: "guard_tripped" }),
      }),
      NOW,
    );
    expect(pill.tooltip).toBe("disabled because the envelope guard tripped");
  });

  it("shows backing off ahead of the latest outcome", () => {
    const pill = statusPill(
      row({ backoff_until: NOW + 4 * 60_000, latest: snapshot("timeout") }),
      NOW,
    );
    expect(pill.kind).toBe("backoff");
    expect(pill.label).toBe("backing off (next in 4 min)");
  });

  it("rounds a sub-minute cooldown up to one minute", () => {
    const pill = statusPill(row({ backoff_until: NOW + 5_000 }), NOW);
    expect(pill.label).toBe("backing off (next in 1 min)");
  });

  it("ignores a cooldown that has already expired", () => {
    const pill = statusPill(
      row({ backoff_until: NOW - 1, latest: snapshot("ok") }),
      NOW,
    );
    expect(pill.kind).toBe("outcome");
    expect(pill.label).toBe("ok");
  });

  it("renders each outcome with its own wording", () => {
    const cases: Array<[Outcome, string]> = [
      ["ok", "ok"],
      ["no_usage_data", "no data — log in?"],
      ["parse_error", "parse error"],
      ["spawn_error", "spawn error"],
      ["timeout", "timeout"],
      ["guard_tripped", "guard tripped"],
    ];
    for (const [outcome, label] of cases) {
      const pill = statusPill(row({ latest: snapshot(outcome) }), NOW);
      expect(pill.kind).toBe("outcome");
      expect(pill.label).toBe(label);
    }
  });

  it("shows no data yet when the account has never been polled", () => {
    const pill = statusPill(row({ latest: null }), NOW);
    expect(pill.kind).toBe("pending");
    expect(pill.label).toBe("not polled yet");
  });

  it("carries the stored error as the tooltip for a failure", () => {
    const pill = statusPill(row({ latest: snapshot("parse_error") }), NOW);
    expect(pill.tooltip).toBe("boom");
  });
});
```

- [ ] **Step 12: Run the test to verify it fails** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
npm test
```

Expect `Failed to resolve import "./pill"`.

- [ ] **Step 13: Write the status pill** — create `src/lib/pill.ts`:

```ts
import type { AccountRow, Outcome } from "./types";

export type PillKind = "disabled" | "backoff" | "outcome" | "pending";

export interface Pill {
  kind: PillKind;
  label: string;
  tooltip?: string;
  /** Only set for an outcome pill, so the row can open the failure detail. */
  outcome?: Outcome;
  snapshotId?: number;
}

const OUTCOME_LABELS: Record<Outcome, string> = {
  ok: "ok",
  no_usage_data: "no data — log in?",
  parse_error: "parse error",
  spawn_error: "spawn error",
  timeout: "timeout",
  guard_tripped: "guard tripped",
};

/**
 * Precedence, highest first: disabled, then backing off, then the latest
 * outcome. Purely presentational: every input comes from the backend.
 */
export function statusPill(row: AccountRow, now: number): Pill {
  if (!row.account.enabled) {
    const tooltip =
      row.account.disabled_reason === "guard_tripped"
        ? "disabled because the envelope guard tripped"
        : "disabled by you";
    return { kind: "disabled", label: "disabled", tooltip };
  }

  if (row.backoff_until !== null && row.backoff_until > now) {
    const minutes = Math.max(1, Math.ceil((row.backoff_until - now) / 60_000));
    return {
      kind: "backoff",
      label: `backing off (next in ${minutes} min)`,
    };
  }

  if (row.latest === null) {
    return { kind: "pending", label: "not polled yet" };
  }

  return {
    kind: "outcome",
    label: OUTCOME_LABELS[row.latest.outcome],
    tooltip: row.latest.error ?? undefined,
    outcome: row.latest.outcome,
    snapshotId: row.latest.id,
  };
}
```

- [ ] **Step 14: Write the failing test for the header banner** — create `src/lib/banner.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { bannerFor } from "./banner";
import type { Account, AccountRow, Dashboard } from "./types";

function account(over: Partial<Account> = {}): Account {
  return {
    id: "a1",
    label: "claude3",
    config_dir: "/home/josh/.claude3",
    enabled: true,
    disabled_reason: null,
    is_default: false,
    created_at: 0,
    ...over,
  };
}

function row(over: Partial<AccountRow> = {}): AccountRow {
  return { account: account(), latest: null, backoff_until: null, ...over };
}

function dash(over: Partial<Dashboard> = {}): Dashboard {
  return {
    accounts: [row()],
    gate: "idle",
    busy: false,
    halted: null,
    stalled_at: null,
    binary: { path: "/home/josh/.local/bin/claude", source: "local_bin" },
    interval_secs: 60,
    ...over,
  };
}

describe("bannerFor", () => {
  it("shows the halt banner ahead of everything else", () => {
    const b = bannerFor(
      dash({
        halted: "guard_tripped:1700000000000",
        stalled_at: 1,
        binary: { path: null, source: null },
        accounts: [row({ account: account({ enabled: false }) })],
      }),
    );
    expect(b?.kind).toBe("halted");
    expect(b?.text).toBe(
      "Polling halted: a /usage call reached the model (see log). Clear only after confirming the Claude Code version/flags",
    );
    expect(b?.action).toBe("clear_halt");
  });

  it("shows the stalled banner next", () => {
    const b = bannerFor(
      dash({ stalled_at: 1_700_000_000_000, binary: { path: null, source: null } }),
    );
    expect(b?.kind).toBe("stalled");
    expect(b?.text.startsWith("Poller stalled at ")).toBe(true);
    expect(b?.text.endsWith(" — recovered")).toBe(true);
  });

  it("shows the missing binary banner next", () => {
    const b = bannerFor(dash({ binary: { path: null, source: null } }));
    expect(b?.kind).toBe("no_binary");
    expect(b?.text).toBe("Claude binary not found — set it in Settings");
    expect(b?.action).toBe("open_settings");
  });

  it("shows the no-enabled-accounts banner when every account is disabled", () => {
    const b = bannerFor(
      dash({ accounts: [row({ account: account({ enabled: false }) })] }),
    );
    expect(b?.kind).toBe("no_accounts");
    expect(b?.text).toBe("Polling paused: no enabled accounts");
  });

  it("shows the active banner with the configured gap", () => {
    const b = bannerFor(dash({ gate: "active", interval_secs: 60 }));
    expect(b?.kind).toBe("active");
    expect(b?.text).toBe("Claude running — polling 60 s after each cycle");
  });

  it("shows the idle banner otherwise", () => {
    const b = bannerFor(dash({ gate: "idle" }));
    expect(b?.kind).toBe("idle");
    expect(b?.text).toBe("Idle — will resume when Claude Code starts");
  });

  it("treats an empty account list as no enabled accounts", () => {
    const b = bannerFor(dash({ accounts: [] }));
    expect(b?.kind).toBe("no_accounts");
  });
});
```

- [ ] **Step 15: Run the test to verify it fails** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
npm test
```

Expect `Failed to resolve import "./banner"`.

- [ ] **Step 16: Write the banner helper** — create `src/lib/banner.ts`:

```ts
import type { Dashboard } from "./types";

export type BannerKind =
  | "halted"
  | "stalled"
  | "no_binary"
  | "no_accounts"
  | "active"
  | "idle";

export interface Banner {
  kind: BannerKind;
  text: string;
  action?: "clear_halt" | "open_settings";
  tone: "error" | "warn" | "info";
}

function clockOf(epochMs: number): string {
  const d = new Date(epochMs);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

/**
 * Header state, first match wins. The no-enabled-accounts case is a purely
 * presentational derivation over the returned rows, not a usage computation.
 */
export function bannerFor(dashboard: Dashboard): Banner | null {
  if (dashboard.halted !== null) {
    return {
      kind: "halted",
      tone: "error",
      action: "clear_halt",
      text:
        "Polling halted: a /usage call reached the model (see log). " +
        "Clear only after confirming the Claude Code version/flags",
    };
  }

  if (dashboard.stalled_at !== null) {
    return {
      kind: "stalled",
      tone: "warn",
      text: `Poller stalled at ${clockOf(dashboard.stalled_at)} — recovered`,
    };
  }

  if (dashboard.binary.path === null) {
    return {
      kind: "no_binary",
      tone: "warn",
      action: "open_settings",
      text: "Claude binary not found — set it in Settings",
    };
  }

  const anyEnabled = dashboard.accounts.some((r) => r.account.enabled);
  if (!anyEnabled) {
    return {
      kind: "no_accounts",
      tone: "warn",
      text: "Polling paused: no enabled accounts",
    };
  }

  if (dashboard.gate === "active") {
    return {
      kind: "active",
      tone: "info",
      text: `Claude running — polling ${dashboard.interval_secs} s after each cycle`,
    };
  }

  return {
    kind: "idle",
    tone: "info",
    text: "Idle — will resume when Claude Code starts",
  };
}
```

- [ ] **Step 17: Run every frontend test and the type check** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
npm test
npx tsc --noEmit
```

Expect 36 passing tests and no type errors.

- [ ] **Step 18: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 22: add frontend DTO types and the pure presentational helpers

Adds the TypeScript mirrors of every backend DTO with no any, plus the four
pure helpers the UI renders through: the reset countdown clamped at resets
now, the live last-updated formatter, the sparkline path builder that turns
missing hours into breaks rather than zeros, the status pill precedence,
and the first-match-wins header banner. All are Vitest covered.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 23: React UI

**Files:** Create `src/hooks/useDashboard.ts`, `src/components/Header.tsx`, `src/components/Sparkline.tsx`, `src/components/AccountsTable.tsx`, `src/components/Settings.tsx`, `src/components/FailureDetail.tsx`, `src/styles.css`; Modify `src/App.tsx`, `src/main.tsx`
**Interfaces:** Consumes: every helper and type from Task 22, and the command names from Task 19. Produces: the rendered window. No new pure logic, so per D14 there are no new Vitest suites here; the gate is `npx tsc --noEmit` plus `npm run build` plus a manual smoke run.

- [ ] **Step 1: Write the dashboard hook** — create `src/hooks/useDashboard.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import type { Dashboard, HistoryPoint } from "../lib/types";

const DEBOUNCE_MS = 250;
const TICK_MS = 1000;

interface UseDashboard {
  dashboard: Dashboard | null;
  history: Record<string, HistoryPoint[]>;
  now: number;
  error: string | null;
  refetch: () => void;
}

/**
 * Events are refetch triggers only: the payloads are ignored and the whole
 * dashboard is re-read through commands, so a missed event can never leave
 * the UI wrong. `now` ticks once a second purely for relative-time text.
 */
export function useDashboard(): UseDashboard {
  const [dashboard, setDashboard] = useState<Dashboard | null>(null);
  const [history, setHistory] = useState<Record<string, HistoryPoint[]>>({});
  const [now, setNow] = useState<number>(() => Date.now());
  const [error, setError] = useState<string | null>(null);
  const pending = useRef<ReturnType<typeof setTimeout> | null>(null);

  const load = useCallback(async (): Promise<void> => {
    try {
      const next = await invoke<Dashboard>("get_dashboard");
      setDashboard(next);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const loadHistory = useCallback(async (): Promise<void> => {
    try {
      const current = await invoke<Dashboard>("get_dashboard");
      const entries = await Promise.all(
        current.accounts.map(async (row) => {
          const points = await invoke<HistoryPoint[]>("get_history", {
            accountId: row.account.id,
          });
          return [row.account.id, points] as const;
        }),
      );
      setHistory(Object.fromEntries(entries));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const refetch = useCallback((): void => {
    if (pending.current !== null) {
      clearTimeout(pending.current);
    }
    pending.current = setTimeout(() => {
      pending.current = null;
      void load();
    }, DEBOUNCE_MS);
  }, [load]);

  useEffect(() => {
    void load();
    void loadHistory();
  }, [load, loadHistory]);

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), TICK_MS);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;

    const attach = async (): Promise<void> => {
      const names = ["usage:updated", "gate:changed", "poller:stalled"];
      for (const name of names) {
        const off = await listen(name, () => refetch());
        if (cancelled) {
          off();
        } else {
          unlisteners.push(off);
        }
      }
      const offCycle = await listen("cycle:finished", () => {
        refetch();
        void loadHistory();
      });
      if (cancelled) {
        offCycle();
      } else {
        unlisteners.push(offCycle);
      }
    };

    void attach();
    return () => {
      cancelled = true;
      for (const off of unlisteners) {
        off();
      }
      if (pending.current !== null) {
        clearTimeout(pending.current);
        pending.current = null;
      }
    };
  }, [refetch, loadHistory]);

  return { dashboard, history, now, error, refetch };
}
```

- [ ] **Step 2: Write the header** — create `src/components/Header.tsx`:

```tsx
import { invoke } from "@tauri-apps/api/core";
import type { JSX } from "react";
import { bannerFor } from "../lib/banner";
import type { Dashboard } from "../lib/types";

interface Props {
  dashboard: Dashboard;
  onOpenSettings: () => void;
  onChanged: () => void;
  onError: (message: string) => void;
}

export function Header({
  dashboard,
  onOpenSettings,
  onChanged,
  onError,
}: Props): JSX.Element {
  const banner = bannerFor(dashboard);

  const run = async (command: string): Promise<void> => {
    try {
      await invoke(command);
      onChanged();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <header className="header">
      {banner !== null && (
        <div className={`banner banner-${banner.tone}`} role="status">
          <span>{banner.text}</span>
          {banner.action === "clear_halt" && (
            <button type="button" onClick={() => void run("clear_halt")}>
              Clear halt
            </button>
          )}
          {banner.action === "open_settings" && (
            <button type="button" onClick={onOpenSettings}>
              Open Settings
            </button>
          )}
        </div>
      )}
      <div className="header-actions">
        <button
          type="button"
          onClick={() => void run("poll_now")}
          disabled={dashboard.busy}
        >
          {dashboard.busy ? "Refreshing…" : "Refresh now"}
        </button>
        <button type="button" onClick={onOpenSettings}>
          Settings
        </button>
      </div>
    </header>
  );
}
```

- [ ] **Step 3: Write the sparkline component** — create `src/components/Sparkline.tsx`:

```tsx
import type { JSX } from "react";
import { buildSparklinePaths } from "../lib/sparkline";
import type { HistoryPoint } from "../lib/types";

interface Props {
  points: HistoryPoint[];
  width?: number;
  height?: number;
}

/**
 * Hand-rolled SVG, no chart library. Each sub-path is one contiguous run of
 * hours; gaps between runs are simply not drawn.
 */
export function Sparkline({
  points,
  width = 120,
  height = 24,
}: Props): JSX.Element {
  const paths = buildSparklinePaths(points, width, height);

  if (paths.length === 0) {
    return <span className="sparkline-empty">—</span>;
  }

  return (
    <svg
      className="sparkline"
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      role="img"
      aria-label="weekly usage, last 7 days"
    >
      {paths.map((d, i) => (
        <path
          key={`${i}-${d.slice(0, 16)}`}
          d={d}
          fill="none"
          strokeWidth={1.5}
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      ))}
    </svg>
  );
}
```

- [ ] **Step 4: Write the accounts table** — create `src/components/AccountsTable.tsx`:

```tsx
import { invoke } from "@tauri-apps/api/core";
import type { JSX } from "react";
import { useState } from "react";
import { formatAgo, formatCountdown } from "../lib/format";
import { statusPill } from "../lib/pill";
import type { AccountRow, HistoryPoint } from "../lib/types";
import { Sparkline } from "./Sparkline";

interface Props {
  rows: AccountRow[];
  history: Record<string, HistoryPoint[]>;
  now: number;
  onChanged: () => void;
  onError: (message: string) => void;
  onShowFailure: (snapshotId: number) => void;
}

function Bar({ pct }: { pct: number }): JSX.Element {
  const tone = pct >= 90 ? "red" : pct >= 70 ? "amber" : "green";
  return (
    <div className="bar" title={`${pct}%`}>
      <div className={`bar-fill bar-${tone}`} style={{ width: `${pct}%` }} />
      <span className="bar-label">{pct}%</span>
    </div>
  );
}

export function AccountsTable({
  rows,
  history,
  now,
  onChanged,
  onError,
  onShowFailure,
}: Props): JSX.Element {
  const [renaming, setRenaming] = useState<string | null>(null);
  const [draftLabel, setDraftLabel] = useState<string>("");

  const call = async (
    command: string,
    args: Record<string, unknown>,
  ): Promise<void> => {
    try {
      await invoke(command, args);
      onChanged();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <table className="accounts">
      <thead>
        <tr>
          <th>Account</th>
          <th>Session</th>
          <th>Week (all)</th>
          <th>Per model</th>
          <th>Last 7 days</th>
          <th>Updated</th>
          <th>Status</th>
          <th>Actions</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => {
          const pill = statusPill(row, now);
          const session = row.latest?.session ?? null;
          const week = row.latest?.week_all ?? null;
          const models = row.latest?.week_models ?? [];
          return (
            <tr key={row.account.id} className={row.account.enabled ? "" : "row-off"}>
              <td>
                {renaming === row.account.id ? (
                  <form
                    onSubmit={(e) => {
                      e.preventDefault();
                      setRenaming(null);
                      void call("update_account", {
                        id: row.account.id,
                        label: draftLabel,
                      });
                    }}
                  >
                    <input
                      value={draftLabel}
                      onChange={(e) => setDraftLabel(e.target.value)}
                      autoFocus
                    />
                  </form>
                ) : (
                  <span title={row.account.config_dir}>
                    {row.account.label}
                    {row.account.is_default && <em className="tag">default</em>}
                  </span>
                )}
              </td>
              <td>
                {session !== null ? (
                  <>
                    <Bar pct={session.pct} />
                    <div className="sub">
                      {formatCountdown(session.resets_at, now)}
                    </div>
                  </>
                ) : (
                  "—"
                )}
              </td>
              <td>{week !== null ? <Bar pct={week.pct} /> : "—"}</td>
              <td>
                {models.length === 0
                  ? "—"
                  : models.map((m) => (
                      <span key={m.label} className="model">
                        {m.label} {m.pct}%
                      </span>
                    ))}
              </td>
              <td>
                <Sparkline points={history[row.account.id] ?? []} />
              </td>
              <td>{formatAgo(row.latest?.taken_at ?? null, now)}</td>
              <td>
                <button
                  type="button"
                  className={`pill pill-${pill.kind}`}
                  title={pill.tooltip}
                  disabled={pill.snapshotId === undefined || pill.outcome === "ok"}
                  onClick={() => {
                    if (pill.snapshotId !== undefined) {
                      onShowFailure(pill.snapshotId);
                    }
                  }}
                >
                  {pill.label}
                </button>
              </td>
              <td className="actions">
                <button
                  type="button"
                  onClick={() =>
                    void call("update_account", {
                      id: row.account.id,
                      enabled: !row.account.enabled,
                    })
                  }
                >
                  {row.account.enabled ? "Disable" : "Enable"}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setRenaming(row.account.id);
                    setDraftLabel(row.account.label);
                  }}
                >
                  Rename
                </button>
                <button
                  type="button"
                  onClick={() => void call("open_login", { id: row.account.id })}
                >
                  Log in
                </button>
                <button
                  type="button"
                  onClick={() => void call("remove_account", { id: row.account.id })}
                >
                  Remove
                </button>
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
```

- [ ] **Step 5: Write the settings panel** — create `src/components/Settings.tsx`:

```tsx
import { invoke } from "@tauri-apps/api/core";
import type { JSX } from "react";
import { useEffect, useState } from "react";
import type { BinaryInfo, UserSettings } from "../lib/types";

interface Props {
  binary: BinaryInfo;
  onClose: () => void;
  onChanged: () => void;
  onError: (message: string) => void;
}

export function Settings({
  binary,
  onClose,
  onChanged,
  onError,
}: Props): JSX.Element {
  const [settings, setSettings] = useState<UserSettings | null>(null);
  const [newPath, setNewPath] = useState<string>("");

  useEffect(() => {
    const load = async (): Promise<void> => {
      try {
        setSettings(await invoke<UserSettings>("get_settings"));
      } catch (e) {
        onError(e instanceof Error ? e.message : String(e));
      }
    };
    void load();
  }, [onError]);

  if (settings === null) {
    return <aside className="settings">Loading…</aside>;
  }

  // The backend rejects out-of-range values; on rejection the previous value
  // is kept by simply re-reading what the backend still holds.
  const save = async (next: UserSettings): Promise<void> => {
    const previous = settings;
    setSettings(next);
    try {
      await invoke("set_settings", { settings: next });
      onChanged();
    } catch (e) {
      setSettings(previous);
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  const call = async (
    command: string,
    args: Record<string, unknown> = {},
  ): Promise<void> => {
    try {
      await invoke(command, args);
      onChanged();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <aside className="settings">
      <h2>Settings</h2>

      <label>
        Poll gap (seconds, 10–3600)
        <input
          type="number"
          min={10}
          max={3600}
          value={settings.interval_secs}
          onChange={(e) =>
            void save({ ...settings, interval_secs: Number(e.target.value) })
          }
        />
      </label>

      <label>
        Poll timeout (seconds, 5–120)
        <input
          type="number"
          min={5}
          max={120}
          value={settings.timeout_secs}
          onChange={(e) =>
            void save({ ...settings, timeout_secs: Number(e.target.value) })
          }
        />
      </label>

      <label>
        Claude binary override
        <input
          type="text"
          placeholder="leave blank to auto-detect"
          value={settings.claude_binary}
          onChange={(e) =>
            void save({ ...settings, claude_binary: e.target.value })
          }
        />
      </label>
      <p className="sub">
        detected: {binary.path ?? "none"}
        {binary.source !== null && ` (${binary.source})`}
      </p>

      <label>
        <input
          type="checkbox"
          checked={settings.close_to_tray}
          onChange={(e) =>
            void save({ ...settings, close_to_tray: e.target.checked })
          }
        />
        Close to tray
      </label>

      <label>
        <input
          type="checkbox"
          checked={settings.launch_at_login}
          onChange={(e) =>
            void save({ ...settings, launch_at_login: e.target.checked })
          }
        />
        Launch at login
      </label>

      <label>
        <input
          type="checkbox"
          checked={settings.log_level === "debug"}
          onChange={(e) =>
            void save({
              ...settings,
              log_level: e.target.checked ? "debug" : "info",
            })
          }
        />
        Debug logging
      </label>

      <h3>Accounts</h3>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void call("add_account", { configDir: newPath });
          setNewPath("");
        }}
      >
        <input
          type="text"
          placeholder="path to a config directory"
          value={newPath}
          onChange={(e) => setNewPath(e.target.value)}
        />
        <button type="submit">Add account</button>
      </form>
      <button type="button" onClick={() => void call("rescan_profiles")}>
        Rescan profiles
      </button>

      <h3>Diagnostics</h3>
      <button type="button" onClick={() => void call("open_log_dir")}>
        Open log folder
      </button>
      <p className="sub">History is kept for 30 days and then pruned.</p>

      <button type="button" onClick={onClose}>
        Close
      </button>
    </aside>
  );
}
```

- [ ] **Step 6: Write the failure detail modal** — create `src/components/FailureDetail.tsx`:

```tsx
import { invoke } from "@tauri-apps/api/core";
import type { JSX } from "react";
import { useEffect, useState } from "react";
import type { RawSnapshot } from "../lib/types";

interface Props {
  snapshotId: number;
  onClose: () => void;
}

export function FailureDetail({ snapshotId, onClose }: Props): JSX.Element {
  const [data, setData] = useState<RawSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const load = async (): Promise<void> => {
      try {
        setData(await invoke<RawSnapshot>("get_snapshot_raw", { snapshotId }));
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    };
    void load();
  }, [snapshotId]);

  return (
    <div className="modal" role="dialog" aria-modal="true">
      <div className="modal-body">
        <h2>Poll failure</h2>
        {error !== null && <p className="error">{error}</p>}
        {data !== null && (
          <>
            <h3>Error</h3>
            <pre>{data.error ?? "(none recorded)"}</pre>
            <h3>Raw output</h3>
            <pre className="raw">{data.raw ?? "(no output captured)"}</pre>
          </>
        )}
        <button type="button" onClick={onClose}>
          Close
        </button>
      </div>
    </div>
  );
}
```

- [ ] **Step 7: Write the root component** — replace `src/App.tsx`:

```tsx
import type { JSX } from "react";
import { useState } from "react";
import { AccountsTable } from "./components/AccountsTable";
import { FailureDetail } from "./components/FailureDetail";
import { Header } from "./components/Header";
import { Settings } from "./components/Settings";
import { useDashboard } from "./hooks/useDashboard";
import "./styles.css";

export default function App(): JSX.Element {
  const { dashboard, history, now, error, refetch } = useDashboard();
  const [showSettings, setShowSettings] = useState(false);
  const [failureId, setFailureId] = useState<number | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const showError = (message: string): void => {
    setToast(message);
    window.setTimeout(() => setToast(null), 6000);
  };

  if (dashboard === null) {
    return (
      <main className="app">
        <p>{error ?? "Loading…"}</p>
      </main>
    );
  }

  return (
    <main className="app">
      <Header
        dashboard={dashboard}
        onOpenSettings={() => setShowSettings(true)}
        onChanged={refetch}
        onError={showError}
      />

      <AccountsTable
        rows={dashboard.accounts}
        history={history}
        now={now}
        onChanged={refetch}
        onError={showError}
        onShowFailure={(id) => setFailureId(id)}
      />

      {showSettings && (
        <Settings
          binary={dashboard.binary}
          onClose={() => setShowSettings(false)}
          onChanged={refetch}
          onError={showError}
        />
      )}

      {failureId !== null && (
        <FailureDetail
          snapshotId={failureId}
          onClose={() => setFailureId(null)}
        />
      )}

      {toast !== null && <div className="toast">{toast}</div>}
    </main>
  );
}
```

- [ ] **Step 8: Write the entry point and styles** — replace `src/main.tsx`:

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

const container = document.getElementById("root");
if (container === null) {
  throw new Error("missing #root element");
}

ReactDOM.createRoot(container).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

Then create `src/styles.css`:

```css
:root {
  color-scheme: light dark;
  --bg: #ffffff;
  --fg: #16181d;
  --muted: #5f6672;
  --line: #e2e5ea;
  --green: #2ea043;
  --amber: #d29622;
  --red: #d73333;
  font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
  font-size: 14px;
}

@media (prefers-color-scheme: dark) {
  :root {
    --bg: #14161a;
    --fg: #e9ecf1;
    --muted: #9aa2b1;
    --line: #262b33;
  }
}

body {
  margin: 0;
  background: var(--bg);
  color: var(--fg);
}

.app {
  padding: 16px;
}

.header {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-bottom: 16px;
}

.banner {
  padding: 8px 12px;
  border-radius: 6px;
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}
.banner-error {
  background: rgba(215, 51, 51, 0.15);
  border: 1px solid var(--red);
}
.banner-warn {
  background: rgba(210, 150, 34, 0.15);
  border: 1px solid var(--amber);
}
.banner-info {
  background: transparent;
  border: 1px solid var(--line);
  color: var(--muted);
}

.header-actions {
  display: flex;
  gap: 8px;
}

.accounts {
  width: 100%;
  border-collapse: collapse;
}
.accounts th,
.accounts td {
  text-align: left;
  padding: 8px;
  border-bottom: 1px solid var(--line);
  vertical-align: top;
}
.row-off {
  opacity: 0.55;
}

.bar {
  position: relative;
  width: 110px;
  height: 14px;
  background: var(--line);
  border-radius: 7px;
  overflow: hidden;
}
.bar-fill {
  height: 100%;
}
.bar-green {
  background: var(--green);
}
.bar-amber {
  background: var(--amber);
}
.bar-red {
  background: var(--red);
}
.bar-label {
  position: absolute;
  inset: 0;
  font-size: 11px;
  text-align: center;
  line-height: 14px;
}

.sub {
  color: var(--muted);
  font-size: 12px;
  margin-top: 4px;
}

.model {
  display: inline-block;
  margin-right: 8px;
  white-space: nowrap;
}

.sparkline path {
  stroke: var(--green);
}
.sparkline-empty {
  color: var(--muted);
}

.pill {
  border: 1px solid var(--line);
  border-radius: 999px;
  padding: 2px 10px;
  background: transparent;
  color: inherit;
  font-size: 12px;
}
.pill-outcome {
  border-color: var(--amber);
}
.pill-disabled {
  opacity: 0.7;
}

.tag {
  margin-left: 6px;
  font-size: 11px;
  color: var(--muted);
}

.actions button {
  margin-right: 4px;
}

.settings {
  margin-top: 24px;
  padding: 16px;
  border: 1px solid var(--line);
  border-radius: 8px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  max-width: 520px;
}
.settings label {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.modal {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: grid;
  place-items: center;
}
.modal-body {
  background: var(--bg);
  padding: 16px;
  border-radius: 8px;
  max-width: 720px;
  max-height: 80vh;
  overflow: auto;
}
.modal-body pre {
  white-space: pre-wrap;
  word-break: break-word;
  background: var(--line);
  padding: 8px;
  border-radius: 6px;
}
.raw {
  max-height: 320px;
  overflow: auto;
}

.toast {
  position: fixed;
  right: 16px;
  bottom: 16px;
  padding: 10px 14px;
  border-radius: 6px;
  background: var(--red);
  color: #fff;
}
.error {
  color: var(--red);
}
```

- [ ] **Step 9: Type check and build** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
npx tsc --noEmit
npm test
npm run build
```

Expect no type errors, 36 passing tests and a clean build. If `JSX` cannot be imported as a type from `react`, replace `import type { JSX } from "react"` with `import type { ReactElement } from "react"` and change every return type from `JSX.Element` to `ReactElement`.

- [ ] **Step 10: Smoke-run the app** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
npm run tauri dev
```

Confirm by eye: the window opens, the accounts table lists the discovered config dirs, the header shows the idle or active banner, the tray icon appears with a tooltip, closing the window hides it to the tray, and the tray Quit exits. Stop with Ctrl+C.

- [ ] **Step 11: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 23: add the React UI

Adds the event-driven dashboard hook that treats events as refetch triggers
and ignores their payloads, the header banner with its clear-halt and
settings actions, the accounts table with bars, per-model cells, the
hand-rolled SVG sparkline and the precedence-ordered status pill, the
settings panel that reverts to the previous value when the backend rejects
input, and the failure detail modal showing the stored error and raw
output.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Task 24: README, manual integration checklist and the final gate

**Files:** Create `README.md`; Modify `.gitignore` if anything untracked remains
**Interfaces:** Consumes: everything. Produces: the documented manual integration procedure from the last row of spec §10, and a recorded green run of all four gates.

- [ ] **Step 1: Write the README** — create `README.md`:

```markdown
# Claude Usage Tracker

A task-manager-style desktop app for Claude subscription usage: the 5-hour
session window, the weekly all-models window and every weekly per-model
window, for one or more accounts, refreshed while Claude Code is running.

Design spec: `docs/2026-09-15-claude-usage-tracker-design.md`
Implementation plan: `docs/superpowers/plans/2026-09-15-claude-usage-tracker.md`

## How it works

An account is a Claude Code config directory. The app polls each enabled
account by spawning the Claude Code binary directly with:

```
claude -p /usage --output-format json --model haiku
       --no-session-persistence --strict-mcp-config
       --permission-prompts none --safe-mode
```

It never goes through a shell, and it strips every `ANTHROPIC_` and `CLAUDE_`
variable from the child environment before setting `CLAUDE_CONFIG_DIR`. An
envelope guard checks that the reply really was a local command; if it ever
looks like a model turn, polling halts globally and the halt survives a
restart until you clear it.

## Requirements

- Rust 1.96 (MSVC toolchain on Windows) and a C compiler, for bundled SQLite
- Node 24
- Linux only: `libayatana-appindicator3-dev` for the tray

## Build and run

```bash
npm ci
npm run tauri dev      # development
npm run tauri build    # release bundle
```

## Gates

All four must be green:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

The single test thread is required: the runner and driver integration tests
configure the `fake_claude` helper through process-wide environment variables.

## Manual integration check

Automated tests never call the real CLI. Run this by hand after any change to
the runner, the parser or the flag set. It spends one `/usage` call per
account, which is free.

1. Confirm the binary is found: open Settings and check "detected: <path>
   (<source>)". It should read `local_bin` on a native install.
2. With a real logged-in config dir enabled (for example `~/.claude3`), press
   **Refresh now**. Within about three seconds the row should show a session
   percentage, a week percentage and any per-model lines.
3. Confirm no session file was written:
   `ls ~/.claude3/projects` should be unchanged.
4. Enable a config dir that is not logged in. Its pill should settle on
   `no data — log in?` and then enter backoff rather than failing every cycle.
5. Start an interactive `claude` session. The header should switch to "Claude
   running — polling N s after each cycle" within one interval. Quit it; the
   header should return to idle after exactly one more poll.
6. Open the log folder from Settings and confirm JSON lines with `outcome`,
   `duration_ms` and `trigger` fields.
7. Toggle debug logging and confirm the process-check DEBUG lines appear
   without a restart.

**The Git Bash hazard is deliberately not reproduced.** Passing `/usage`
through an MSYS shell path-mangles it into a real prompt and spends quota. The
app spawns the binary directly precisely so this cannot happen; the envelope
guard is the detector for it, not a thing to test by triggering.

## Where things live

- Database: `<app data dir>/usage.sqlite` (30 days of snapshots, pruned daily)
- Logs: `<app log dir>/claude-usage-tracker.*.log` (7 daily files)
- Poll working directory: `<app data dir>/poll-cwd`
- Login helper script: `<app data dir>/login/` (emptied at startup)

The exact app data and log directories are logged at startup.
```

- [ ] **Step 2: Run every gate** — run each command and confirm it is green:

```bash
cd /c/Users/isjav/ClaudeUsageTracker/src-tauri
cargo test -- --test-threads=1
cargo clippy --all-targets -- -D warnings
cd /c/Users/isjav/ClaudeUsageTracker
npm test
npm run build
```

- [ ] **Step 3: Scan the tree for leftovers** — confirm no placeholder survived and nothing untracked is missing from `.gitignore`:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
grep -rn "TODO\|FIXME\|unimplemented!\|todo!" src src-tauri/src || echo "no placeholders"
grep -rn "\.unwrap()\|\.expect(" src-tauri/src --include=*.rs | grep -v "#\[cfg(test)\]" | grep -v "mod tests" || echo "review any hits above: they must all be inside test modules"
git status --porcelain
```

Any `unwrap` or `expect` hit outside a `#[cfg(test)]` module is a defect; replace it with a `Result` path before continuing.

- [ ] **Step 4: Run the manual integration check** — follow every numbered step in the README section "Manual integration check" against the real `~/.claude3` config dir, and note the observed session and week percentages. If step 3 shows a new session file, stop: a flag in the argv constant is not doing its job.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/isjav/ClaudeUsageTracker
git add -A
git commit -m "$(cat <<'MSG'
Task 24: add the README with the manual integration checklist

Documents the build requirements, the four gates and why they run on a
single test thread, where the database, logs, poll working directory and
login script live, and the by-hand integration procedure that exercises the
real CLI once per account. Records that the Git Bash hazard is deliberately
not reproduced because doing so spends quota.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014sqgoktsDjFqBApr4zsfg2
MSG
)"
```

---

## Self-Review

Written against **spec v5 (final)**. The v5 deltas are folded in as follows: the `DriverStatus` snapshot (§5.1) is defined in Task 12, published in Task 20 and read in Task 19; `Backoff.unexpected_envelope_streak` plus `record() -> Recorded` and `status()` are in Task 12, which moves the §6.3 step 5 escalation out of the runner; the §8 settings split is in Task 9 (`polling_relevant_changed`), Task 19 (applying it) and Task 20 (the watch arm that resets backoff). The §6.3 step 2 rewording is wording only and the guard order in Task 14 already matched it.

### 1. Spec coverage

**§5 Architecture and §5.1 Core types**

| Spec item | Task | Note |
|---|---|---|
| `src-tauri/src` module layout | File Structure section; Tasks 1–21 | `scheduler/triggers.rs` added (not in §5) to hold the trigger set §6.5 describes but does not place in a file |
| `lib.rs` builder, plugins, tray, close→hide, state, scheduler start, shutdown | 21 | |
| `paths.rs` | 6 | |
| `discovery.rs` | 7 | |
| `process.rs` | 5 | |
| `usage/mod.rs` types | 3 | |
| `usage/runner.rs` | 14 (guard, argv, strikes) + 15 (spawn, env, timeout) | Split so the guard is unit-tested without spawning |
| `usage/parser.rs` | 4 | |
| `scheduler/machine.rs` | 12 (+ `preview_manual` in 19) | |
| `scheduler/driver.rs` | 20 | |
| `store/mod.rs`, `schema.rs`, `accounts.rs`, `snapshots.rs`, `settings.rs` | 8, 8, 10, 11, 9 | |
| `commands.rs` | 19 | |
| `tray.rs` | 16 (pure) + 21 (icons, apply, event sink) | |
| `login.rs` | 18 | |
| `logging.rs` | 17 | |
| React `App`, `Header`, `AccountsTable`, `Sparkline`, `Settings`, `FailureDetail`, `hooks/useDashboard` | 23 | |
| Data flow, events as refetch triggers only, 250 ms debounce, 1 s tick | 20 (emit) + 23 (hook) | |
| `Account`, `DisabledReason`, `Window`, `Parsed`, `PollOutcome`, `Snapshot`, `SnapshotDto` | 3 | |
| `DriverStatus { gate, busy, stalled_at, backoff_until }` in `Arc<Mutex<..>>` | defined 12, published 20, read 19 | Defined in `machine.rs` because `Machine::status` builds it from `Gate` and the backoff map |
| The six `outcome` strings, everything but `ok` is a failure | 3 | |

**§6 Component contracts**

| Spec item | Task |
|---|---|
| 6.1 `find_claude_binary` precedence, `.exe`-only on Windows, D3 WARN, INFO on selection | 7 |
| 6.1 `enumerate_profiles`, markers, `dunce::canonicalize`, label, D17 order | 7 |
| 6.1 first start seeds enabled; rescan adds disabled | 10 |
| 6.2 `is_claude_running`, `refresh_processes_specifics` kinds, native + npm match, DEBUG timing | 5 |
| 6.3 argv constant | 14 |
| 6.3 D15 environment, cwd, stdin null, pipes, `kill_on_drop`, `CREATE_NO_WINDOW` | 15 |
| 6.3 PID published as `exclude_pid` | 15 (publish) + 20 (consume) |
| 6.3 timeout, kill, wait | 15 |
| 6.3 envelope guard rules 1–4 | 14 |
| 6.3 step 5 five-strike escalation (`Backoff.unexpected_envelope_streak`, decided in `record`, returned as `Recorded::Escalate`) | 12 (counter + decision) + 20 (cycle task acts on it) |
| 6.3 halt order: flag, log, outcome, disable, abort cycle | 20 (and Task 15 stays silent so the ERROR line cannot precede the flag) |
| 6.3 `Skip(Halted)` for every trigger including Manual | 12 |
| 6.3 red banner and Clear halt button | 22 (`bannerFor`) + 23 (Header) |
| 6.3 non-zero exit and non-JSON stdout → `spawn_error` | 15 |
| 6.4 parser: line iteration, detection predicate, R1/R2/R3 order, duplicates, pct range, reset grammar, zones, DST, year wrap | 4 |
| 6.5 `Gate`, `Trigger`, `Decision`, `SkipReason`, `Backoff`, `Recorded`, `decide` rules 0–6, `begin_cycle`, RAII token, `record`, `reset_all_backoff`, `status`, `cycle_age` | 12 |
| 6.5 snake_case wire forms for every enum | 12 (+ 3 for `DisabledReason` and outcomes) |
| 6.5 driver: cycle task, coalescing `Notify`, accumulating `AccountChanged`, select loop, deadline, settings watch, watchdog arm disabled when idle, shutdown | 20 |
| 6.5 binary re-stat before every decision | 20 |
| 6.5 serial polls in D17 order, per-account persist + emit + tray, `cycle:finished` | 20 |
| 6.5 `ExitRequested` with `prevent_exit`, explicit kill and wait, second exit allowed through | 20 (kill/wait) + 21 (exit hook) |
| 6.6 pragmas, `Mutex<Connection>`, `spawn_blocking` callers, migrations | 8 (+ 19 for the shared `blocking` helper and the command wrappers, + 20 for every driver-side store call) |
| 6.6 schema and both indexes | 8 |
| 6.6 `latest_per_account`, `history`, `prune` + `incremental_vacuum` | 11 |
| 6.6 settings keys, `polling_halted`, `launch_at_login` not stored | 9 (+ 19 for the autostart write-through) |
| D16 / §8 split: only the three polling keys reach the scheduler | 9 (`polling_relevant_changed`) + 19 (applies the split) + 20 (watch arm resets backoff) |
| 6.7 tray menu, left-click, close→hide, `tray_state`, tooltip format | 16 (pure) + 21 (wiring) |
| 6.8 login script per OS, quoting rules, directory emptied at startup | 18 (+ 21 for the startup empty) |
| 6.9 JSON layer, daily rotation, 7 files, `reload::Layer`, per-account fields | 17 (+ 20 for the poll fields) |

**§7 UI**

| Spec item | Task |
|---|---|
| Header banner, first match wins, all six states | 22 (`bannerFor`) + 23 |
| Refresh now, Settings buttons | 23 |
| Accounts table: label, session bar + countdown, week-all, per-model cells, sparkline, last updated, status pill | 23 |
| Countdown clamped at "resets now", em dash when absent | 22 |
| Sparkline gaps as path breaks | 22 |
| Pill precedence disabled > backing off > outcome | 22 |
| Row actions enable/disable, rename, log in, remove | 23 |
| Settings panel incl. detected binary, add by path, rescan, toggles, log folder, 30 d retention note | 23 |
| Failure detail on a non-`ok` pill | 23 |
| Out-of-range input rejected by the backend, previous value kept | 9 (reject) + 23 (revert) |

**§8 Command surface** — all thirteen commands plus `AppError { code, message }` and the four events are in Task 19 (commands) and Task 20 (events). `get_dashboard` carries `gate`, `busy`, `halted`, `stalled_at`, `binary`, `interval_secs` and per-row `backoff_until`; `stalled_at` lives in `DriverStatus` and is cleared on the next reaped cycle.

**§10 Testing plan, row by row**

| Row | Task | Gap or deviation |
|---|---|---|
| parser (18 listed cases) | 4 | None. All present including 150 %, CRLF, 12am/12pm, Dec→Jan, DST gap, ambiguous, unknown zone, R3-not-stealing-R2 |
| runner — guard table in order, turn evidence, advisory in isolation, shape cases | 14 | None |
| runner — five-strike escalation | 12 (counter, per-account tracking, reset rules) + 20 (end-to-end) | v5 moved the counter out of the runner into `Backoff.unexpected_envelope_streak`, so the unit tests live in the machine suite and the driver suite proves five unclassifiable envelopes really do halt the poller |
| runner — halt flag persisted before the cycle abort, with a failing store after step 1 | 20 | **Deviation:** implemented against a `HaltSink` trait with a recording/failing fake rather than a fault-injected real `Store`. Same assertion, no need for a failure-injecting SQLite layer |
| runner — parser fixture for 12am/12pm | 4 | **Placement gap:** lives in the parser suite, not the runner suite. The runner has no separate clock, so duplicating it there would test nothing new |
| runner — non-zero exit, exit 0 + non-JSON, timeout via fake binary, exact argv, env sanitisation | 13 + 15 | None |
| scheduler/machine — `Halted` beats everything, `NoEnabledAccounts` vs `AllBackedOff`, gate does not move on a skip, the four Timer combos, busy before process check, `NoBinary`, Manual/Startup ignore gate, `AccountChanged` intersection, `Timer` with `None`, final-poll-once, backoff schedule and resets, RAII on panic, `cycle_age` | 12 | None |
| scheduler/machine — guard trip mid-cycle aborts the remaining accounts (polls after the trip: zero) | 20 | **Placement gap:** this is driver behaviour, so it is asserted in the driver integration test by counting persisted snapshots, not in the machine unit tests |
| discovery | 7 | None |
| store | 8, 9, 10, 11 | None. Cascade, canonical uniqueness and halt-survives-reopen all present |
| process | 5 | None |
| tray | 16 | None |
| commands | 19 | None. Adds the v5 settings split (three keys publish, three do not) and proves the dashboard and `poll_now` read only the published snapshot |
| frontend | 22 | None. Banner precedence added beyond the listed four |
| scheduler/driver | 20 | None. Adds `publish_status` coverage: gate/busy/backoff copied out, `stalled_at` preserved, and a live test that a `record` is always followed by a publish |
| integration (manual, README) | 24 | None. The Git Bash hazard is explicitly not reproduced |

**Deliberate spec resolutions, each recorded as a code comment in the task that makes it:**

1. **`PollOutcome::Timeout` carries the timeout in seconds** (Task 3). §5.1 declares the variant without a payload but requires the error text `"timed out after {n}s"`; the payload is the only way to produce that text from `error_text()`.
2. **`AccountChanged(ids)` with an empty intersection skips as `no_enabled_accounts`** (Task 12). §6.5 rule 5 defines `AllBackedOff` as "enabled accounts exist and every one is in cooldown", which is false here, and `AccountChanged` bypasses the backoff filter entirely.
3. **The live `Child` is owned by the cycle task, not a shared `Mutex<Option<Child>>`** (Task 15). Awaiting the child's exit while holding that mutex would deadlock the shutdown path that wants the same mutex to kill it. Shutdown reaches the child through the `CancellationToken`; the child is still only killed through its own handle and the PID is still only published as `exclude_pid`.
4. **`preview_manual` is a free function over `DriverStatus`** (Task 12). `poll_now` must report `skipped:<reason>` without consuming a trigger or mutating backoff, and v5 forbids commands from reading `Machine`, so the preview takes the published snapshot. A Manual trigger bypasses the gate and backoff, so only rules 0–3 apply and the preview is exact.
5. **Tray icons are drawn at runtime from `Level`** (Task 21) rather than shipped as five image assets, so the icon set cannot drift from the enum and nothing extra enters the bundle.
6. **Linux tray default item**: Tauri 2 has no "default menu item" API, so "Open" is simply the first menu item and left-click is handled where the platform delivers it (Task 21). This matches §6.7's intent; it is not a separate API call.
7. **`UNEXPECTED_ENVELOPE_PREFIX` and `is_unexpected_envelope` live in `usage/mod.rs`** (Task 3), not in `runner.rs`. Both the guard that produces a shape-class message and `Machine::record`, which counts the streak, need them, and the machine must not depend on the runner.
8. **`Machine::status` returns `stalled_at: None`** (Task 12). The watchdog owns that value, so the driver merges the previous one in when it publishes (Task 20).
9. **`backoff_until` in the snapshot is not filtered by `now`** (Task 12). The snapshot is written when the driver acts and read later by the UI, which already ignores an elapsed deadline; filtering at write time would hide a cooldown that is still in force.
10. **The binary path is not part of `DriverStatus`** (Task 19). §5.1 defines `DriverStatus` as scheduler state only, so `(path, source)` sits in its own `BinarySlot` beside it, written by the driver's pre-decision re-stat and read by `get_dashboard` and `poll_now`.
11. **Signature evolution from v4 to v5, recorded so a later reader does not re-propose the v4 shape.** `Machine::record` was `(&str, OutcomeKind, i64) -> ()` and the five-strike counter was a separate `EnvelopeStrikes { HashMap<String, u32> }` in `runner.rs` held by the driver as a second `Arc<Mutex<..>>`. v5 makes it `(&str, &PollOutcome, i64) -> Recorded`, folds the counter into `Backoff.unexpected_envelope_streak: u8`, and deletes `EnvelopeStrikes` entirely. `record` needs the whole outcome, not just its kind, because only the message distinguishes a shape-class spawn error from any other spawn error. Alongside it, commands lost their `Machine` handle: `Core` used to hold `machine: SharedMachine` and `core_get_dashboard` / `core_poll_now` / `core_update_account` / `core_set_settings` all locked it. They now read the published `DriverStatus` instead, and the only backoff resets left are the ones the spec puts inside `decide` and inside the driver's settings-watch arm. There is exactly one `record` signature and one `MAX_ENVELOPE_STRIKES` (`u8`, in `machine.rs`) in the whole plan.
12. **`run_usage` never logs a guard trip** (Task 15). §6.3 fixes the order as persist the halt flag, then log the raw envelope, then persist the outcome. An ERROR line inside the runner would land before the flag reached disk and misreport that order, so the runner returns silently and carries the bytes on `RunResult::raw`. The single trip log site is `StoreHalt::log_envelope` in Task 20, and a Task 15 test pins the raw carrier so the rule stays safe.
13. **The driver reaches the store only through `commands::blocking`** (Task 20). §6.6 says every `Store` method is synchronous and the mutex is never held across an `await`; with `busy_timeout=5000` a contended statement can park its thread for five seconds, which must not be a runtime worker. That is why `settings`, `halted`, `enabled` and `decide_and_maybe_run` are async and why `StoreHalt` owns its data: the whole four-step halt sequence moves into one blocking hop.
14. **The watchdog select arm carries the guard `if cycle_running`** (Task 20), which is `live.is_some()`. It is the same predicate as §6.5's `machine.cycle_age(now).is_some()` but does not take the machine lock inside a `select!` precondition.

### 2. Placeholder scan

Run over the finished plan:

```bash
grep -nE "TBD|FIXME|\bTODO\b|similar to Task|write tests for the above|add error handling" \
  docs/superpowers/plans/2026-09-15-claude-usage-tracker.md
```

One hit, and it is the literal `grep` pattern written inside Task 24 Step 3, which is the tree scan the implementer runs. No task defers work to another task by reference; every code step carries its own complete code, repeated rather than cross-referenced.

### 3. Shared type and name consistency

| Name | Defined in | Consumed by |
|---|---|---|
| `AppError` / `AppResult` / `{code, message}` | Task 2 | every task from 5 onwards; TS mirror `AppErrorShape` in Task 22 |
| `Window`, `ModelWindow`, `Parsed` | Task 3 | 4, 11, 16; TS `Win`, `ModelWindow` in Task 22 |
| `PollOutcome` (incl. `Timeout(u32)`) | Task 3 | 4, 11, 15, 20 |
| `OutcomeKind` + the six wire strings | Task 3 | 11, 12, 20; TS `Outcome` in Task 22 |
| `Snapshot`, `SnapshotDto` | Task 3 | 11, 16, 19; TS `SnapshotDto` in Task 22 |
| `Account`, `DisabledReason` | Task 3 | 10, 16, 19; TS `Account`, `DisabledReason` in Task 22 |
| `BinarySource`, `Found`, `Candidate` | Task 7 | 10, 19, 20; TS `BinarySource` in Task 22 |
| `Gate` | Task 12 | 19 (`Dashboard.gate`), 20; TS `Gate` in Task 22 |
| `DriverStatus`, `Backoff`, `Recorded`, `MAX_ENVELOPE_STRIKES`, `preview_manual` | Task 12 | 19 (`Core.status`, `core_poll_now`, `core_get_dashboard`), 20 (`publish_status`, `run_cycle`), 21 (initial value) |
| `Trigger` | Task 12 | 19, 20 |
| `SkipReason` | Task 12 | 19 (`poll_now`), 20 |
| `Decision` | Task 12 | 20 |
| `Machine`, `SharedMachine`, `CycleToken`, `begin_cycle`, `lock_machine` | Task 12 | 20 only (the driver is the sole owner) |
| `UNEXPECTED_ENVELOPE_PREFIX`, `is_unexpected_envelope` | Task 3 | 12 (`record`), 14 (`shape`), 15 (tests) |
| `USAGE_ARGV`, `GuardVerdict`, `check_envelope`, `env_names_to_strip` | Task 14 | 15, 20 |
| `RunResult`, `run_usage` | Task 15 | 20 |
| `polling_relevant_changed` | Task 9 | 19 (`core_set_settings`) |
| `Store`, `with_conn`, `with_conn_mut`, `open`, `open_in_memory` | Task 8 | 9, 10, 11, 19, 20, 21 |
| `Store::get_raw` / `set_raw` / `stored_settings` / `save_settings` / `polling_halted` / `set_polling_halted` / `clear_polling_halted` | Task 9 | 19, 20, 21 |
| `Store::list_accounts` / `enabled_account_ids` / `account_by_id` / `add_account` / `update_account` / `remove_account` / `mark_guard_tripped` / `seed_accounts_if_empty` / `rescan_accounts` | Task 10 | 16, 19, 20, 21 |
| `Store::insert_snapshot` / `latest_per_account` / `history` / `prune` / `snapshot_raw`, `HistoryPoint`, `RETENTION_MS` | Task 11 | 16, 19, 20, 21; TS `HistoryPoint` in Task 22 |
| `UserSettings`, `validate_settings`, the six clamp constants | Task 9 | 19, 20, 23; TS `UserSettings` in Task 22 |
| `Level`, `tray_state`, `level_rgb`, `icon_rgba`, `should_hide_on_close`, `MENU_*` | Tasks 16 and 21 | 21 |
| `Core`, `SharedCore`, `BinarySlot`, `lock_status`, `lock_binary`, `Dashboard`, `AccountRow`, `BinaryInfo`, `RawSnapshot` | Task 19 | 20, 21; TS `Dashboard`, `AccountRow`, `BinaryInfo`, `RawSnapshot` in Task 22 |
| `blocking<T, F>` (the shared `spawn_blocking` hop) | Task 19 | 19 (every command wrapper), 20 (every driver store call) |
| `Triggers` | Task 19 | 19, 20 |
| `EventSink`, `ProcessProbe`, `BinaryProbe`, `Driver`, `deadline_for`, `watchdog_limit_ms`, `publish_status`, `HaltSink`, `perform_halt`, `PRUNE_INTERVAL_MS` | Task 20 | 21 |
| Command names `get_dashboard`, `get_history`, `poll_now`, `add_account`, `update_account`, `remove_account`, `rescan_profiles`, `get_settings`, `set_settings`, `clear_halt`, `open_login`, `open_log_dir`, `get_snapshot_raw` | Task 19 | 21 (`generate_handler!`), 23 (`invoke`) |
| Event names `usage:updated`, `cycle:finished`, `gate:changed`, `poller:stalled` | Task 20 (`EventSink`) / Task 21 (`TauriEvents`) | 23 (`useDashboard`) |
| `formatCountdown`, `formatAgo`, `buildSparklinePaths`, `statusPill`, `bannerFor` | Task 22 | 23 |
