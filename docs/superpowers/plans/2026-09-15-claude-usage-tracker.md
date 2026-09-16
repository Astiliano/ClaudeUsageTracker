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
cd /c/Users/josh/ClaudeUsageTracker
npm create tauri-app@latest -- app --template react-ts --manager npm --identifier io.github.astiliano.claudeusagetracker --yes
```

This writes into `./app`. Move its contents up one level and delete the empty dir:

```bash
cd /c/Users/josh/ClaudeUsageTracker
cp -r app/. .
rm -rf app
```

If `--yes` is rejected by the installed CTA version, re-run with `-y` in its place; if `--identifier` is rejected, drop it and edit `identifier` in `src-tauri/tauri.conf.json` by hand to `io.github.astiliano.claudeusagetracker`.

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
cd /c/Users/josh/ClaudeUsageTracker
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
  "identifier": "io.github.astiliano.claudeusagetracker",
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
cd /c/Users/josh/ClaudeUsageTracker/src-tauri
cargo check --all-targets
```

Expect a clean finish. If `rusqlite` fails to link, the MSVC Build Tools C compiler is missing — install it before continuing; do not switch off `bundled`.

- [ ] **Step 13: Verify the frontend builds** — run:

```bash
cd /c/Users/josh/ClaudeUsageTracker
npm run build
```

Expect `dist/` to be written with no TypeScript errors.

- [ ] **Step 14: Commit** — run:

```bash
cd /c/Users/josh/ClaudeUsageTracker
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
cd /c/Users/josh/ClaudeUsageTracker/src-tauri
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
cd /c/Users/josh/ClaudeUsageTracker/src-tauri
cargo test --lib error::
cargo clippy --all-targets -- -D warnings
```

Expect 5 passing tests and no clippy warnings.

- [ ] **Step 5: Commit** — run:

```bash
cd /c/Users/josh/ClaudeUsageTracker
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
