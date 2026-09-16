# Claude Usage Tracker — Design Spec

Date: 2026-09-15
Status: **spec v5 — FINAL (hostile + consistency review passed)** (see §15 Review log)

## 1. Goal

A cross-platform desktop app that shows, task-manager style, the subscription
usage (5-hour session window, weekly window, weekly per-model windows) for one
or more Claude accounts, refreshing live while Claude Code is in use, with
history so burn rate over the week is visible.

Non-goals for v1 are listed in §13.

## 2. Research findings (verified locally 2026-09-15, Claude Code 2.1.273)

### 2.1 Data source

| Data source | Covers | Official? | Verdict |
|---|---|---|---|
| `claude -p "/usage" --output-format json` | Subscription 5h / 7d / 7d-per-model %, reset times | Yes (documented `/usage`, documented print mode) | **Use this** |
| Claude Code statusLine JSON `rate_limits` | Same numbers | Yes | Only fires during a live session; not needed |
| Admin API usage/cost reports | API orgs only | Yes | Not applicable to subscriptions |
| Enterprise Analytics API | Enterprise admins | Yes | Not applicable |
| Internal endpoint / claude.ai OAuth | Subscription | **No** | Rejected — unsupported, ToS-gray |
| Scraping claude.ai in a webview | Subscription | No | Rejected — brittle |

### 2.2 Behaviour of the `/usage` print command

- Handled locally: `num_turns: 0`, `total_cost_usd: 0`, `duration_api_ms: 0`,
  and a **`local_command: "usage"`** field in the JSON envelope. The result
  text is in `result`. A real model turn (the hazard below) has **no**
  `local_command` field.
- Works per account via `CLAUDE_CONFIG_DIR` (tested `~/.claude`, `~/.claude2`,
  `~/.claude3`).
- **Not-logged-in / empty config dir:** exit code 0, `is_error: false`,
  `local_command: "usage"`, but `result` is a cost summary
  (`Total cost: $0.0000 …`) instead of the usage report. Detection must be by
  content, not exit code.
- **Timing** (Windows 11, native binary): 4.0–4.2 s baseline; **~3 s** with
  the flag set below. Empty config dir: 0.9 s.
- **Footprint per call (baseline flags):** writes a session `.jsonl` under
  `<config>/projects/<cwd-slug>/`, rewrites `.claude.json` (+ a backup in
  `backups/`, which Claude Code prunes to 5), runs user SessionStart hooks,
  loads plugins and MCP servers. `/usage` is **not** appended to
  `history.jsonl`.
- **Flag set that removes the footprint** (verified: still returns
  `local_command: "usage"`, no session file written, stdout is UTF-8, LF-only,
  no BOM):

  ```
  claude -p /usage --output-format json --model haiku
         --no-session-persistence --strict-mcp-config
         --permission-prompts none --safe-mode
  ```

  `--safe-mode` disables hooks/plugins/MCP but keeps OAuth auth. `--bare`
  must NOT be used: it never reads OAuth credentials. `--tools ""` was
  considered and dropped: it is inert for a local command and an empty
  argument is the token most likely to be lost by an intermediate layer,
  which would shift the remaining flags into a positional prompt.
- **Shell hazard:** from Git Bash / MSYS the argument `/usage` is path-mangled
  into `C:/Program Files/Git/usage`, which reaches the model as a prompt and
  **spends a real turn** (observed: ~$0.75 of Fable usage). The app therefore
  spawns the binary directly (no shell of any kind) and applies the envelope
  guard in §6.3. The guard is a **detector, not a preventer**: by the time it
  trips, one turn has been spent; its job is to make sure that never happens
  twice.
- **Output shape** (unversioned plain text):

  ```
  You are currently using your subscription to power your Claude Code usage

  Current session: 15% used · resets Sep 16, 3:30am (America/Los_Angeles)
  Current week (all models): 4% used · resets Sep 21, 8am (America/Los_Angeles)
  Current week (Fable): 5% used · resets Sep 21, 8am (America/Los_Angeles)

  What's contributing to your limits usage?
  Approximate, based on local sessions on this machine — …
  Last 24h · 8834 requests · 9 sessions
    99% of your usage came from subagent-heavy sessions
  …
  ```

  Observed variants: `Current session: 0% used` with **no reset clause**; the
  per-model label ("Fable") varies by plan/model and there may be zero, one
  or more per-model lines. Reset times have no year and carry an IANA zone
  name; minutes are omitted on the hour (`8am` vs `3:30am`). The separator is
  U+00B7 (middle dot).

### 2.3 Config-dir layout (what identifies an account)

| Dir | Files present | Meaning |
|---|---|---|
| `~/.claude` | `.credentials.json`, `settings.json`, `projects/`… (no `.claude.json`; the default dir keeps it at `~/.claude.json`) | default, logged in |
| `~/.claude2`, `~/.claude3` | `.credentials.json`, `.claude.json`, `settings.json` | logged in |
| `~/.claude-free`, `~/.claude-kilofree` | `.claude.json`, `projects/`, no credentials | profile, not logged in |
| `~/.claude-flow` | `update-state.json` only | **not** a config dir |

Discovery rule: a directory `~/.claude*` is a candidate account iff it
contains at least one of `.credentials.json`, `.claude.json`, `settings.json`.

### 2.4 Process facts

- Native install: process name `claude.exe` (Windows) / `claude` (macOS,
  Linux), exe under `~/.local/bin/`. `~/.local/bin/claude.exe` is a full copy
  of the current version (231 MB), not a launcher.
- Interactive sessions show as `claude.exe --dangerously-skip-permissions …`;
  no persistent daemon process was observed (a `~/.claude/daemon/` state dir
  exists but had no live process).
- npm install form: the process is `node`/`node.exe` running
  `…/node_modules/@anthropic-ai/claude-code/cli.js` (path fragment from the
  package layout; **verify-on-first-encounter** — no npm install on this
  machine).

## 3. Existing setup this builds on

Josh runs multiple accounts as one Claude Code binary plus one config dir per
account, switched via `CLAUDE_CONFIG_DIR` (PowerShell functions `claude2` /
`claude3`). The app adopts this model directly: **an account is a config
dir.** No in-app OAuth. "Log in" opens a visible terminal with
`CLAUDE_CONFIG_DIR` set, running `claude /login`.

## 4. Decisions

| # | Decision | Rationale |
|---|---|---|
| D1 | Data source: `claude -p /usage` with the §2.2 flag set | Only official, subscription-capable source |
| D2 | Stack: **Tauri 2.11** (Rust backend) + React 18/TypeScript (Vite). No UI framework; hand-rolled SVG sparklines | Small binary; backend owns all logic; frontend is a thin view |
| D3 | Binary discovery: settings override → `~/.local/bin/claude[.exe]` → PATH lookup. **Only real executables are accepted**; on Windows a `.cmd`/`.bat` shim is rejected with the message "npm shim not supported; install the native build (`claude install`) or point Settings at `claude.exe`" | Spawning a batch file re-parses the command line through `cmd.exe`, the same hazard class as §2.2 |
| D4 | Accounts: default = `CLAUDE_CONFIG_DIR` from the app's environment else `~/.claude`; auto-discover per §2.3; user can rename/enable/disable/remove/add-path | Mirrors how the user already works |
| D5 | Refresh interval = **idle gap between the end of one cycle and the start of the next**, user-adjustable, min 10 s, max 3600 s, default 60 s | A gap (not a fixed rate) means 10 s can never become continuous spawning even with many accounts |
| D6 | Process gate (§6.5): poll only while a Claude Code process is running, plus one final poll after it stops. Manual refresh, startup, and account add/enable bypass the gate | No wasted polls overnight; numbers still fresh when it matters |
| D7 | One cycle at a time. Every poll path goes through the same `decide` → `begin_cycle` pair (§6.5); anything arriving while a cycle runs is **skipped, never queued** | Bounded resource use; one state machine |
| D8 | Every poll outcome is persisted and displayed; **raw text is stored with every snapshot** (success or failure) so any snapshot can be re-parsed after a parser fix | Failure visibility, not silence. Storage: ~1.1 KB/poll; at 60 s, 8 h/day, 3 accounts ≈ 1.6 MB/day ≈ 50 MB per retention window — acceptable |
| D9 | Tray icon reflects the **worst percentage across all enabled accounts and all their windows** (session, week-all, every per-model line); tooltip lists every enabled account | Worst-of is what you act on |
| D10 | History retention: snapshot rows older than **30 days** are deleted at startup and every 24 h, followed by `PRAGMA incremental_vacuum` (DB opened with `auto_vacuum=INCREMENTAL`) so the file actually shrinks. No separate raw pruning | Bounds DB size; D8 stays true for the whole window. Worst case at the 10 s floor, 3 accounts, Claude running 24 h/day: 8640 cycles × 3 × 1.1 KB ≈ 28.5 MB/day ≈ **855 MB** per window — the user chose that gap and the gate makes 24 h/day unrealistic; the default is ~50 MB |
| D11 | The "What's contributing" section is **not parsed or displayed** in v1; it is preserved inside `raw` | YAGNI; unstable heuristics text |
| D12 | Threshold notifications: **deferred to v1.1** | Not core to the "task manager" value |
| D13 | Logging: `tracing` JSON lines to a daily-rotating file in the app log dir; runtime level toggle via `reload::Handle`; "Open log folder" menu item | Debuggable in production without adding code; firehose one switch away |
| D14 | Frontend testing: Vitest on pure helpers only. No E2E in v1 | Logic lives in Rust; the webview is presentational |
| D15 | Child environment is **sanitised**: inherit the parent environment, remove every variable whose name starts with `ANTHROPIC_` or `CLAUDE_` (**compared case-insensitively** — Windows env names are case-insensitive), then set `CLAUDE_CONFIG_DIR` | An inherited `ANTHROPIC_API_KEY` would turn a guard miss into metered spend; `CLAUDE_CODE_USE_BEDROCK` etc. would change the auth path |
| D16 | Per-account **failure backoff**: on the k-th consecutive non-`ok` outcome (k ≥ 1) set `next_allowed = now + min(900 s, 60 s × 2^(k−1))`; reset to zero on `ok`, manual refresh, account edit, or a change to a **polling-relevant** setting (`interval_secs`, `timeout_secs`, `claude_binary`) — `close_to_tray`, `launch_at_login`, `log_level` do not touch the scheduler | A logged-out account or missing binary must not write an identical failure row every cycle; fixing the binary path or interval is a deliberate "try again" |
| D17 | Account order: default account first, then by label (case-insensitive). No manual reordering in v1 | YAGNI (drag-to-reorder cut in review) |

Defaults D9–D12 were chosen by the implementing agent and can be changed
without architectural impact.

## 5. Architecture

```
src-tauri/src/
  lib.rs             Tauri builder: plugins (single-instance FIRST, autostart,
                     opener), tray, window close→hide, state init, scheduler start,
                     shutdown hook
  paths.rs           home dir, app data/log dirs, default config dir, poll cwd
  discovery.rs       find_claude_binary(); enumerate_profiles(home)
  process.rs         is_claude_running(&mut System, exclude_pid) -> bool
  usage/
    mod.rs           PollOutcome, Parsed, Window, Snapshot, SnapshotDto
    runner.rs        run_usage(binary, config_dir, timeout) -> RunResult
    parser.rs        parse_usage(result_text, now) -> PollOutcome
  scheduler/
    machine.rs       pure state machine (gate, busy, backoff) — no I/O
    driver.rs        tokio loop: sleep-gap timer, settings watch, cycle runner
  store/
    mod.rs           Store: Mutex<Connection>, every public fn is sync and is
                     called through spawn_blocking by callers
    schema.rs        migrations (PRAGMA user_version), pragmas
    accounts.rs / snapshots.rs / settings.rs
  commands.rs        #[tauri::command] surface (§8)
  tray.rs            tray_state(latest, halted) -> (Level, tooltip)  [pure] + apply
  login.rs           open_terminal_for_login(binary, config_dir) per OS
  logging.rs         tracing init, reload handle
src/                 React: App, Header, AccountsTable, Sparkline, Settings,
                     FailureDetail, hooks/useDashboard
```

Data flow: `driver` → `runner` → `parser` → `store` → `emit("usage:updated")`
→ React refetches `get_dashboard` (debounced 250 ms). **Events are refetch
triggers only**: the frontend ignores event payloads and re-reads state
through commands (`get_dashboard` carries `gate`, `halted`, `stalled_at`),
so a missed event can never leave the UI wrong. Payloads exist for logs and
tests. The frontend never
computes usage percentages or outcomes; it renders backend DTOs and runs a
local 1 s tick for "resets in" / "N s ago" text.

### 5.1 Core types

```rust
pub struct Account { id: String /*uuid v4*/, label: String,
                     config_dir: PathBuf /*canonicalised*/, enabled: bool,
                     disabled_reason: Option<DisabledReason>, is_default: bool,
                     created_at: i64 /*epoch ms*/ }
pub enum DisabledReason { User, GuardTripped }

pub struct Window { pct: u8, resets_at: Option<i64 /*epoch ms UTC*/> }

pub struct Parsed {
  session: Window,
  week_all: Window,
  week_models: Vec<(String /*label*/, Window)>,   // 0..n lines, in output order
}

pub enum PollOutcome {
  Ok(Parsed),
  NoUsageData,          // envelope fine, result text is not a usage report
  ParseError(String),   // usage report detected, a required line failed
  SpawnError(String),   // binary missing/not executable, non-zero exit,
                        // stdout not JSON — message = stderr/stdout tail
  Timeout(u32),         // killed after timeout_secs (carried); error = "timed out after {n}s"
  GuardTripped(String), // §6.3 violated — halts the whole poller
}

pub struct Snapshot { id: i64, account_id: String, taken_at: i64 /*epoch ms*/,
                      outcome: PollOutcome, raw: Option<String>,
                      duration_ms: u32 }

/// Published by the driver into AppState after every `decide()` and every
/// `record()`; the ONLY way code outside driver.rs reads scheduler state
/// (Machine's own fields are never read across tasks, so nothing can drift).
pub struct DriverStatus { gate: Gate, busy: bool, stalled_at: Option<i64>,
                          backoff_until: HashMap<AccountId, i64> }
// AppState holds Arc<Mutex<DriverStatus>>; get_dashboard copies it.

/// Wire shape sent to the frontend (flattened; built by store from a row).
pub struct SnapshotDto { id, account_id, taken_at, outcome: &'static str,
                         session: Option<Window>, week_all: Option<Window>,
                         week_models: Vec<{label, pct, resets_at}>,
                         error: Option<String>, duration_ms }
```

`outcome` string values (used identically in the DB column, the DTO, logs,
and the UI): `ok`, `no_usage_data`, `parse_error`, `spawn_error`, `timeout`,
`guard_tripped`. For backoff (D16) and the status pill, every value except
`ok` is a failure.

## 6. Component contracts

### 6.1 `discovery.rs`

- `find_claude_binary(settings) -> Result<Found { path, source }, NotFound>`:
  settings override if set → `~/.local/bin/claude.exe|claude` →
  first PATH hit. `source` ∈ `override | local_bin | path`. A candidate is
  accepted only if it is a regular file, is executable (Unix mode bit), and
  on Windows has extension `.exe`. `.cmd`/`.bat` candidates are skipped and
  the skip is logged at WARN with the D3 message. Logged at INFO with the
  chosen path and source.
- `enumerate_profiles(home) -> Vec<Candidate { config_dir, label }>`: every
  directory `home/.claude*` satisfying §2.3, canonicalised
  (`dunce::canonicalize` so Windows paths have no `\\?\` prefix). Label = dir
  name without the leading dot. Order per D17.
- First start seeds the accounts table with every candidate **enabled**: the
  app has nothing to show otherwise, the profiles are the user's own, and
  not-logged-in ones settle into `no_usage_data` + backoff. Later
  `rescan_profiles` adds new candidates **disabled** (`disabled_reason =
  user`) because a rescan is an explicit user action whose result they will
  review, and silently starting to poll a new dir would be a surprise.

### 6.2 `process.rs`

- `is_claude_running(sys: &mut System, exclude_pid: Option<Pid>) -> bool`.
  Refresh with
  `sys.refresh_processes_specifics(ProcessesToUpdate::All, true,
   ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet)
                                .with_cmd(UpdateKind::OnlyIfNotSet))`.
- Match if pid ≠ `exclude_pid` and either (a) process name equals `claude` or
  `claude.exe` case-insensitively, or (b) name is `node`/`node.exe` and any
  cmd arg contains `@anthropic-ai/claude-code` (either slash). Other
  scripted `claude -p` users are counted: they consume quota too.
- The pure matcher `matches_claude(name, cmd) -> bool` is unit-tested; the
  check duration and matched PIDs are logged at DEBUG.

### 6.3 `usage/runner.rs`

- Spawns the binary **directly** via `tokio::process::Command` with argv
  `["-p", "/usage", "--output-format", "json", "--model", "haiku",
   "--no-session-persistence", "--strict-mcp-config",
   "--permission-prompts", "none", "--safe-mode"]` — held in one `const`.
  Environment per D15 (stripped names logged at INFO on first poll per
  account, DEBUG thereafter). cwd = `<app_data_dir>/poll-cwd` (created if
  missing). stdin `null`, stdout+stderr piped, `kill_on_drop(true)`, Windows
  `creation_flags(CREATE_NO_WINDOW)`.
- The child's PID is published to the scheduler (`exclude_pid`) for the life
  of the poll; the `Child` handle stays with the cycle task (§6.5).
- Timeout `settings.timeout_secs` (default 30, min 5, max 120) via
  `tokio::time::timeout`; on expiry `kill().await`, then `wait().await`,
  return `Timeout`.
- **Envelope guard** (evaluated before parsing), in this order:
  1. stdout is not JSON, or has no `type`, or `type != "result"` →
     `SpawnError("unexpected envelope: <reason>")` (backs off; raw stored).
  2. The envelope is a `result`. `local_command` absent or not equal to
     `"usage"` → **`GuardTripped`** (primary turn evidence — a model turn
     has no such field). This is checked before the `num_turns` /
     `result`-presence checks in step 4, so a turn envelope with an odd
     shape can never be misclassified as a shape problem and retried. (Step
     1 runs first by necessity: a non-JSON or non-`result` payload cannot
     carry `local_command` at all; §2.2 shows real turns always produce a
     `result` envelope.)
  3. `num_turns > 0`, or `total_cost_usd > 0`, or non-empty `modelUsage` →
     `GuardTripped` (secondary/advisory: under subscription auth cost can
     read 0 for a billed turn, so these add trips, never excuse one).
  4. `local_command == "usage"` but `num_turns` absent/non-numeric or
     `result` missing → `SpawnError("unexpected envelope: <reason>")`.
  5. **Escalation:** five consecutive `unexpected envelope` spawn errors on
     one account → treated as `GuardTripped("unclassifiable envelope ×5")`,
     because an envelope the app cannot classify is exactly the case where
     it cannot prove no turn was spent. The counter is
     `Backoff.unexpected_envelope_streak: u8` in `machine.rs` (§6.5),
     incremented by `record()` for that error kind and reset to 0 only by a
     non-strike outcome in `record()` — **never** by the backoff resets
     that manual refresh, account edit or a settings change perform (the
     streak is guard evidence; a Refresh click must not erase it); the escalation decision is made in `record()`, which
     returns `Escalate` so the cycle task performs the trip sequence below.
  On a trip, in this order: (1) persist the **global halt**
  (`settings.polling_halted = "guard_tripped:<ts>"`) — the flag is the safety
  property, so it goes to disk first; (2) log the raw envelope at ERROR;
  (3) persist the outcome for that account; (4) **abort the current cycle**
  (remaining accounts are not polled — same binary, same argv, same fault).
  While halted, `decide` returns `Skip(Halted)` for **every** trigger
  including `Manual`. The header shows a red banner "Polling halted: a
  /usage call reached the model (see log). Clear only after confirming the
  Claude Code version/flags" with a "Clear halt" button that calls
  `clear_halt`. The tripped account additionally gets
  `disabled_reason=GuardTripped` so the row explains which account tripped.
- Non-zero exit → `SpawnError(stderr tail, 2 KB max)`. Exit 0 with
  non-JSON stdout → `SpawnError("non-JSON stdout: <tail>")`.
- Success returns `result` text + `duration_ms` to the parser.

### 6.4 `usage/parser.rs` (pure, fixture-tested)

- Input: result text, `now: DateTime<Utc>`. All chrono arithmetic is
  internal; the returned `Window.resets_at` is already epoch ms UTC (`i64`),
  the system-wide timestamp form.
- Lines are iterated with `str::lines()` (tolerates CRLF although §2.2 shows
  LF only), each trimmed of trailing whitespace.
- **Detection predicate:** the text is a usage report iff at least one line
  matches `^Current (session|week)\b`. If not → `NoUsageData`.
- Per line, try in this order and take the **first** match:
  - R1 `^Current session: (\d{1,3})% used(?: · resets (.+?) \((.+)\))?$`
  - R2 `^Current week \(all models\): (\d{1,3})% used(?: · resets (.+?) \((.+)\))?$`
  - R3 `^Current week \((.+?)\): (\d{1,3})% used(?: · resets (.+?) \((.+)\))?$`
  R3 is last because it also matches the all-models line. `·` is U+00B7.
  Lines matching none are ignored (forward compatibility).
- After the pass: missing R1 or R2 → `ParseError("missing <line>")`. A
  duplicated R1/R2 → `ParseError`.
- pct: parse the capture as `u16`; if > 100 → `ParseError("pct out of
  range")`; else store as `u8`.
- Reset clause grammar: `^([A-Z][a-z]{2}) (\d{1,2}), (\d{1,2})(?::(\d{2}))?(am|pm)$`
  (month abbrev, day, 12-hour time with optional minutes; `12am` → 00:xx,
  `12pm` → 12:xx, otherwise `pm` adds 12). Zone = the IANA
  name in parentheses, resolved with `chrono-tz`; unknown zone →
  `ParseError`. Build the local datetime with year = `now` converted to that
  zone; DST gap → first valid instant after the gap; ambiguous → earliest.
  Convert to UTC; if it is more than 30 days before `now` (UTC comparison),
  add one year and rebuild (Dec→Jan wrap).
- Output `PollOutcome::Ok(Parsed)`.

### 6.5 Scheduler

#### `machine.rs` — pure, no clock, no I/O

```rust
pub enum Gate { Idle, Active }
pub struct Machine { gate: Gate, cycle: Option<CycleToken>,
                     backoff: HashMap<AccountId, Backoff> }
pub struct Backoff { consecutive_failures: u32, next_allowed: i64,
                     unexpected_envelope_streak: u8 }
pub enum Recorded { Continue, Escalate /* §6.3 step 5 */ }
pub fn record(&mut self, id: &AccountId, outcome: &PollOutcome, now: i64) -> Recorded
pub fn reset_all_backoff(&mut self)
pub fn status(&self, now: i64) -> DriverStatus   // snapshot for AppState
pub enum Trigger { Timer, Manual, Startup, AccountChanged(Vec<AccountId>) }
pub enum Decision { Run { accounts: Vec<AccountId>, reason: Trigger,
                          gate_transition: Option<Gate> },   // driver emits gate:changed
                    Skip(SkipReason) }
pub enum SkipReason { Halted, Busy, NoBinary, NoEnabledAccounts, GateIdle,
                      AllBackedOff }

pub fn is_busy(&self) -> bool
pub fn decide(&mut self, t: Trigger, claude_running: Option<bool>,
              binary_present: bool, halted: bool, enabled: &[AccountId],
              now: i64) -> Decision
```

`decide` is a pure function of its arguments and the machine's fields; it
returns a *description* of what the driver must do and never performs I/O.
Rules, evaluated in this order:

0. `halted` (global guard halt, §6.3) → `Skip(Halted)` for every trigger.
1. `cycle.is_some()` → `Skip(Busy)`. (The driver calls `is_busy()` before
   spending a process check, so the app's own child can never latch the
   gate.)
2. `!binary_present` → `Skip(NoBinary)` for every trigger.
3. `enabled.is_empty()` → `Skip(NoEnabledAccounts)` for every trigger.
4. Compute the candidate list: `Timer` requires `claude_running == Some(_)`
   (`None` is a driver bug: `debug_assert!`, then `Skip(GateIdle)`);
   Idle&!running → `Skip(GateIdle)`; otherwise all enabled.
   `Manual`/`Startup`: all enabled. `AccountChanged(ids)`: `ids ∩ enabled`.
5. Filter by backoff (D16): drop accounts whose `next_allowed > now` unless
   trigger is `Manual` or `AccountChanged` (both reset that account's
   backoff). Empty → `Skip(AllBackedOff)` — which therefore always means
   "enabled accounts exist and every one is in cooldown".
6. **Only now**, for a `Timer` that runs: Idle&running → gate=Active,
   `gate_transition=Some(Active)`; Active&!running → gate=Idle,
   `gate_transition=Some(Idle)` (the final poll). A skipped decision never
   moves the gate, so the promised final poll cannot be lost to backoff.

`begin_cycle(now) -> CycleToken` sets `cycle = Some{started_at}`; the token
is an RAII guard whose `Drop` calls `end_cycle()`, so a panic or task abort
can never leave the machine busy. (D7's "one entry point" is
`decide` + `begin_cycle`, always called together by the driver.)
`record(account, outcome, now)` updates backoff. `cycle_age(now) ->
Option<Duration>` is the machine's only watchdog contribution.

**Enum wire forms.** Every enum that crosses a DB/log/event/command
boundary serialises as `snake_case`, like `outcome`: `DisabledReason` →
`user` | `guard_tripped`; `SkipReason` → `halted` | `busy` | `no_binary` |
`no_enabled_accounts` | `gate_idle` | `all_backed_off`; `Trigger` → `timer`
| `manual` | `startup` | `account_changed`; `Gate` → `idle` | `active`.

#### `driver.rs`

- **Execution model:** the driver loop never runs a poll inline. A `Run`
  decision spawns a **cycle task** that owns the `CycleToken`, a
  `CancellationToken`, and the current `Child` handle (behind a
  `Child` owned outright by the cycle task — not behind a shared mutex, which
  would deadlock the shutdown path that waits on it; shutdown and the
  watchdog reach the child through the `CancellationToken`). The loop
  therefore stays
  responsive: any trigger arriving during a cycle is decided immediately and
  gets `Skip(Busy)` (D7). Triggers reach the driver through a
  `tokio::sync::Notify` per trigger kind (never an unbounded queue), so five
  Refresh clicks coalesce into at most one pending trigger.
  `AccountChanged` is backed by a `Mutex<HashSet<AccountId>>` that
  **accumulates** ids; the driver drains it into one
  `AccountChanged(ids)` decision, so enabling two accounts in quick
  succession polls both. (`Trigger::AccountChanged` therefore carries
  `Vec<AccountId>`.)
- Loop:
  ```
  select! {
    _ = sleep_until(deadline)        => Timer,
    _ = settings_rx.changed()        => deadline = last_cycle_end + new_gap (clock not restarted);
                                        machine.reset_all_backoff()  (D16),
    Some(t) = trigger_rx.recv()      => t,
    _ = watchdog.tick(), if machine.cycle_age(now).is_some()   (5 s; arm disabled when idle)
                                     => if cycle_age > enabled × timeout_secs + 10 s:
                                         abort cycle task (JoinHandle::abort), kill child via
                                         its handle, log ERROR, emit poller:stalled,
    _ = shutdown.cancelled()         => break,
  }
  ```
  `deadline` is always `last_cycle_end + interval_secs`; the first deadline
  after startup is `startup_cycle_end + interval_secs`. Aborting the cycle
  task drops its `CycleToken`, which clears busy — the driver never clears
  busy by hand, so two cycles can never overlap.
- A stuck **child** never reaches the watchdog: the per-poll timeout kills
  it and records `timeout`. The watchdog exists for bugs in the cycle task
  itself. The header shows "Poller stalled at HH:MM — recovered (see log)"
  until the next successful cycle.
- **Child identity:** the child is killed only through its `Child` handle
  (owned by the cycle task, abortable via `CancellationToken`), never by
  raw PID. The PID is published solely as `exclude_pid` for the process
  check.

- The driver executes what `decide` describes: on `Run` it emits
  `gate:changed` if `gate_transition` is set, calls `begin_cycle`, and
  spawns the cycle task; on `Skip` it logs the reason at WARN (DEBUG for
  `gate_idle` and `busy`, which are routine).
- Startup: `decide(Startup)` → run, then enter the loop above.
- Before a `Timer` decision the driver calls `is_busy()`; only if not busy
  does it run the process check with `exclude_pid` = the live child PID if
  any. Because `is_busy()` is checked first there is no path today where it
  is `Some`; it is pure defense-in-depth against a future caller that skips
  the busy check.
- Binary presence is re-checked (`stat`) before every decision, so a
  first-run "binary not found" state clears as soon as the user fixes
  Settings.
- A cycle polls accounts serially in D17 order; after each account the
  outcome is persisted, backoff recorded, `usage:updated {account_id}` and
  the tray refreshed; `cycle:finished` at the end.
- Shutdown (tray Quit or window close when not close-to-tray): handled in
  `RunEvent::ExitRequested` with `api.prevent_exit()` — cancel the token,
  explicitly `kill().await` + `wait().await` any registered child (do **not**
  rely on `kill_on_drop`: `app.exit()` ends in `process::exit`, which skips
  destructors), wait ≤ 2 s for the cycle task, then `app.exit(0)` again with
  a flag set so the second `ExitRequested` is allowed through.
- A guard trip inside a cycle persists the halt flag first, then aborts the
  remaining accounts of that cycle (§6.3 order).
- Sleep/wake: nothing special in v1; the next `Timer` after wake runs late.
  On macOS the monotonic clock behind `Instant` pauses during suspend, so at
  the 3600 s maximum the first post-wake poll can be up to an hour late.
  Acceptable for v1 (manual refresh works); noted in §13.

### 6.6 `store/`

SQLite via `rusqlite` (bundled), file `<app_data_dir>/usage.sqlite`. Open
with `journal_mode=WAL`, `foreign_keys=ON`, `busy_timeout=5000`,
`auto_vacuum=INCREMENTAL` (set before the first table is created). A single
`Mutex<Connection>`; every public method is synchronous and callers wrap it
in `tauri::async_runtime::spawn_blocking` — the mutex is never held across an
`await`. Migrations via `PRAGMA user_version`.

```sql
accounts(id TEXT PRIMARY KEY, label TEXT NOT NULL,
         config_dir TEXT NOT NULL UNIQUE,          -- canonicalised
         enabled INTEGER NOT NULL, disabled_reason TEXT,   -- NULL|user|guard_tripped
         is_default INTEGER NOT NULL, created_at INTEGER NOT NULL)
settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)
snapshots(id INTEGER PRIMARY KEY,
          account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
          taken_at INTEGER NOT NULL,                -- epoch ms UTC
          outcome TEXT NOT NULL,                    -- §5.1 values
          session_pct INTEGER, session_resets_at INTEGER,
          week_all_pct INTEGER, week_all_resets_at INTEGER,
          week_models TEXT,                         -- JSON [{label,pct,resets_at}]
          error TEXT, raw TEXT, duration_ms INTEGER NOT NULL);
CREATE INDEX snapshots_acct_time ON snapshots(account_id, taken_at DESC, id DESC);
CREATE INDEX snapshots_time ON snapshots(taken_at);
```

Queries: `latest_per_account()` (max `taken_at`, tiebreak max `id`),
`history(account_id, since)` → hourly buckets `[{t, pct}]` of
`max(week_all_pct)` over `ok` rows, **only for hours that have at least one
row** (no zero-filling — the process gate guarantees overnight gaps, which
must render as breaks, not crashes), `prune(now)` deletes rows with
`taken_at < now − 30 d`, logs the count at WARN if > 0, then runs
`PRAGMA incremental_vacuum` (D10). `record` in the machine resets an
account's backoff on `ok`; `reset_all_backoff()` exists for the settings
watch.

Settings keys: `interval_secs`, `timeout_secs`, `claude_binary` (override
path or empty), `close_to_tray` (default true), `log_level`
(`info`|`debug`), `polling_halted` (absent, or `guard_tripped:<epoch ms>`;
survives restarts by design). `launch_at_login` is **not** stored: `get_settings` reads
the autostart plugin's live state and `set_settings` writes through to it.
`set_settings` also applies `log_level` via the reload handle immediately.

### 6.7 Tray (`tray.rs`)

- Menu: Open (default item), Refresh now, Open log folder, Quit. Left-click
  shows + focuses the window where the platform delivers click events
  (Windows, macOS); on Linux AppIndicator the default menu item is the path.
- Window close → hide when `close_to_tray`, else quit.
- `tray_state(latest: &[(Account, Option<SnapshotDto>)], halted: bool) -> (Level, String)`
  (`halted` is read from the same store key `get_dashboard` uses)
  is pure and unit-tested. `Level` ∈ `Halted` (global guard halt set — a
  distinct icon with a warning badge, tooltip "polling halted — guard
  tripped", takes precedence over everything), `Grey` (no enabled account
  has an `ok` snapshot), `Green` (< 70), `Amber` (70–89), `Red` (≥ 90) from
  the max pct across every window of every enabled account's latest `ok`
  snapshot.
  Tooltip: one line per enabled account, `label  S 15% · W 4% · Fable 5%`
  (per-model segments repeated by label, omitted when none; `err` when the
  latest outcome is not `ok`).
- Applied after each account's poll and on account/settings changes.

### 6.8 Login (`login.rs`)

Opens a visible terminal running the discovered binary with `/login` and
`CLAUDE_CONFIG_DIR` set. On every OS the app writes a **script file** to
`<app_data_dir>/login/` (paths are never interpolated into a shell command
line, so `&`, `%`, spaces and quotes in paths cannot break or expand):
- Windows: `login.cmd` containing `@echo off`, `set "CLAUDE_CONFIG_DIR=<dir>"`,
  `"<binary>" /login`, then `pause`; launched with
  `Command::new("cmd.exe")` with
  `raw_arg(format!("/s /c \"start \"\" cmd.exe /k \"{script}\"\""))` — the
  script path is always explicitly double-quoted (Rust's automatic quoting
  only quotes when it sees a space, and cmd would split an unquoted `&`,
  reachable when the username contains one); the path is under
  `app_data_dir`, which the app controls, and it is rejected if it contains
  a `"` (no CREATE_NO_WINDOW). The env var is written with cmd's `set "K=V"` form,
  which takes the value literally including `&` and `%` until expansion is
  attempted — and it is not expanded again because the binary is invoked
  directly by cmd, not through a second shell.
- macOS: `login.command` (`#!/bin/bash`, `export CLAUDE_CONFIG_DIR=<dir
  single-quoted>; exec '<binary>' /login`), `chmod 700`, `open` it.
- Linux: same script as macOS (`login.sh`); first available of
  `gnome-terminal -- <script>`, `konsole -e <script>`, `xfce4-terminal -e
  <script>`, `xterm -e <script>`.
The script directory is emptied at startup and the file is overwritten on
each use, so nothing accumulates. `/login` is safe in this path: no MSYS is
involved and the worst case is an interactive prompt the user can see.
Failure → `AppError { code: "terminal_unavailable", message: <attempted
command> }` shown as a toast.

### 6.9 Logging (`logging.rs`)

`tracing_subscriber` JSON layer → `tracing_appender` daily rotation in
`app_log_dir()`, 7 files kept, filter behind `reload::Layer`. Every log
line concerning an account carries `account_id` and `label`. Poll lines carry
`duration_ms`, `outcome`, `trigger`. Guard trips log the full raw envelope at
ERROR. No credentials are ever read or logged.

## 7. UI

Single window, dark/light follows OS.

- **Header**: state banner, first match wins — "Polling halted: …" (red,
  §6.3, with Clear halt button), "Poller stalled at HH:MM — recovered",
  "Claude binary not found — set it in Settings" (with a button), "Polling
  paused: no enabled accounts" (derived in the frontend as "every returned
  account has `enabled == false`" — a presentational derivation, not a
  usage computation), "Claude running — polling 60 s after each
  cycle", "Idle — will resume when Claude Code starts". Refresh now,
  Settings.
- **Accounts table** (D17 order): label · Session % bar + "resets in 3h 12m"
  (clamped at "resets now" when `resets_at` is in the past; "—" when absent)
  · Week (all) % · one cell per per-model line ("Fable 5%"; "—" if none) ·
  sparkline (hourly max week-all pct, last 7 days; missing hours are path
  breaks, never zeros) · last updated ("42 s ago", live) · status pill. Pill precedence: `disabled` (with reason
  tooltip) > `backing off (next in 4 min)` > latest outcome (`ok` / `no data
  — log in?` / `parse error` / `spawn error` / `timeout` / `guard tripped`).
- **Row actions**: enable/disable, rename, log in, remove.
- **Settings**: interval, timeout, binary path (with "detected: <path>
  (<source>)"), add account by path, rescan profiles, close-to-tray, launch
  at login, debug logging, open log folder, retention note (30 d fixed).
- **Failure detail**: clicking a non-`ok` pill shows the stored error and the
  raw output for that snapshot.
- Invalid settings input (interval < 10 or > 3600, timeout < 5 or > 120) is
  rejected by the backend with `AppError { code: "out_of_range" }`; the
  frontend shows the error and keeps the previous value.

## 8. Tauri command surface

| Command | Args → Result |
|---|---|
| `get_dashboard` | → `{ accounts: [{ account: Account, latest: SnapshotDto?, backoff_until: i64? }], gate, busy, halted: string?, stalled_at: i64?, binary: { path?, source? }, interval_secs }` — cheap; called on every `usage:updated` (debounced). `stalled_at` and `backoff_until` live in the shared `AppState` (`Arc<Mutex<DriverStatus>>`, written by the driver, read by commands), reset on restart; `stalled_at` is set by the watchdog arm and cleared on the next `cycle:finished`; `halted` comes from the store |
| `get_history` | `{account_id}` → `[{t, pct}]` hourly, last 7 days — called once per account on `cycle:finished` and on mount, not per `usage:updated` |
| `poll_now` | → `"started" \| "skipped:<reason>"` |
| `add_account` | `{config_dir}` → Account. Canonicalises; rejects missing dir (`not_found`) or duplicate (`duplicate`) |
| `update_account` | `{id, label?, enabled?}` → Account. `enabled: false` sets `disabled_reason = user`; `enabled: true` clears it and triggers `AccountChanged` |
| `remove_account` | `{id}` → () (cascades snapshots) |
| `rescan_profiles` | → `[Account]` newly added (disabled) |
| `get_settings` / `set_settings` | user-facing settings struct (`interval_secs`, `timeout_secs`, `claude_binary`, `close_to_tray`, `launch_at_login`, `log_level`); clamps rejected with `out_of_range`; a change to `interval_secs`, `timeout_secs` or `claude_binary` publishes on the settings watch (moves the deadline, resets backoff per D16); changes to the other three keys are applied directly (log reload handle, autostart plugin, in-memory flag) and do **not** touch the watch. `polling_halted` is **not** part of this struct and writes to it do **not** touch the watch |
| `clear_halt` | → () — clears `polling_halted` and logs WARN with the previous value. **Does not poll**: every quota-spending action stays a separate, explicit act (the user presses Refresh) |
| `open_login` | `{id}` → () |
| `open_log_dir` | → () via opener plugin |
| `get_snapshot_raw` | `{snapshot_id}` → `{raw?, error?}` |

All commands are `async fn … -> Result<T, AppError>`; `AppError` serialises to
`{code, message}`. Events: `usage:updated {account_id}`, `cycle:finished`,
`gate:changed {gate}`, `poller:stalled {at, cycle_age_ms}`.

## 9. Dependencies

Versions from crates.io/npm as of 2026-09-15 (research agent); the implementer
pins exact versions at scaffold time and `Cargo.lock`/`package-lock.json` are
committed.

Cargo: `tauri 2.11 (features: tray-icon)`, `tauri-build 2.6`,
`tauri-plugin-single-instance 2.4`, `tauri-plugin-autostart 2.5`,
`tauri-plugin-opener 2.x`, `rusqlite 0.40 (bundled)`, `sysinfo 0.39`,
`tokio 1 (time, process, sync, macros)`, `tokio-util` (CancellationToken),
`serde`, `serde_json`, `chrono`, `chrono-tz`, `regex`, `uuid (v4)`,
`dunce`, `dirs`, `thiserror`, `tracing 0.1`, `tracing-subscriber 0.3 (json,
env-filter)`, `tracing-appender 0.2`. Dev: `tempfile`.

npm: `@tauri-apps/cli 2.11`, `@tauri-apps/api 2.11`,
`@tauri-apps/plugin-autostart`, `@tauri-apps/plugin-opener`, `react 18`,
`react-dom`, `typescript`, `vite`, `vitest`. New packages are installed with
`socket npm install` per the house rule.

Build notes: `rusqlite` bundled needs a C compiler (MSVC Build Tools present).
Linux needs `libayatana-appindicator3-dev` for the tray.

## 10. Testing plan (TDD — tests precede code in every module)

| Module | Tests |
|---|---|
| parser | Fixtures: full three-line report; session 0 % without reset; no per-model line; two per-model lines; **missing session line**; **missing week-all line**; not-logged-in cost summary → `no_usage_data`; non-numeric pct → `parse_error`; **150 % → `parse_error`**; `8am` and `3:30am` clauses; Dec→Jan year wrap; DST-gap and ambiguous local times; unknown zone → `parse_error`; unknown extra lines ignored; CRLF input; R3-not-stealing-R2 (the all-models line never appears in `week_models`) |
| runner | Envelope guard table in §6.3 order: turn-evidence cases (missing `local_command`, `local_command: "cost"`, `num_turns: 1`, **missing `local_command` AND missing `num_turns`** → still `guard_tripped`, **advisory rule in isolation: `local_command: "usage"` + `num_turns: 0` + `total_cost_usd: 0.01` → `guard_tripped`**, and the same with non-empty `modelUsage`); shape cases (non-JSON, `type: "system"`, `local_command: "usage"` with missing `num_turns`) → `spawn_error`; five consecutive unexpected-envelope errors escalate to `guard_tripped`; halt flag is persisted before the cycle abort (ordering test with a failing store after step 1); parser fixture for 12am/12pm; non-zero exit → `spawn_error`; exit 0 + non-JSON → `spawn_error`; timeout via a fake binary `src-tauri/src/bin/fake_claude.rs` (modes selected by env: slow / echo-env / emit-fixture; located in tests via `env!("CARGO_BIN_EXE_fake_claude")`, excluded from the shipped bundle) → `timeout` and the child is gone; exact argv constant; env sanitisation (ANTHROPIC_/CLAUDE_ vars removed, `CLAUDE_CONFIG_DIR` set) using a fake binary that echoes its env |
| scheduler/machine | `Halted` beats every trigger incl. `Manual`; guard trip mid-cycle aborts the remaining accounts (polls made after the trip: zero); `NoEnabledAccounts` vs `AllBackedOff` distinguished; gate does not move on a skipped decision (final poll survives backoff); decision table for `Timer` (4 gate×running combos), busy precedes process check, `NoBinary` for every trigger, `Manual` and `Startup` ignore gate, `AccountChanged(ids)` runs `ids ∩ enabled`, `Timer` with `claude_running == None` → `Skip(GateIdle)`, final-poll-once property (running→stopped→stopped = exactly one Run), backoff schedule and reset rules, RAII token clears busy on drop (including panic via `catch_unwind`), `cycle_age` reporting |
| discovery | Temp-home fixtures reproducing §2.3 (incl. exclusion of the `update-state.json`-only dir); label derivation; D17 ordering; binary precedence with a fake PATH; `.cmd` and `.bat` rejected on Windows (one extension check covers both); canonicalisation |
| store | Migrations from empty; latest-per-account with same-ms tiebreak; hourly history buckets (only populated hours); prune at the 30 d boundary; **cascade on account removal (proves `foreign_keys=ON`)**; unique on canonical path; **`polling_halted` survives close + reopen of the store** |
| process | `matches_claude` on synthetic (name, cmd) tuples: native, npm, unrelated node, excluded pid |
| tray | `tray_state`: `Halted` beats everything; grey/green/amber/red thresholds, multi-account worst-of, per-model segments, `err` rendering |
| commands | `add_account` rejects missing/duplicate; `set_settings` boundary values (10/3600, 5/120) accept, one-off values reject; `update_account` enable clears `disabled_reason`, disable sets `user`; `clear_halt` clears the flag, logs, and does not poll |
| frontend | Vitest: countdown formatter (incl. past `resets_at` → "resets now"), "N s ago" formatter, sparkline path builder (incl. gap → separate sub-paths), status pill precedence |
| scheduler/driver | Tokio-based: triggers during a cycle coalesce to one `Skip(Busy)` and never queue; two `AccountChanged` in quick succession poll both accounts; `clear_halt` does not start a poll; settings change moves the deadline without restarting the clock; watchdog aborts a deliberately hung cycle task and busy clears; shutdown kills a live fake child |
| integration (manual, documented in README) | Real poll against `~/.claude3`; Git Bash hazard reproduction is **not** run (costs quota) |

Gate: `cargo test` + `cargo clippy -D warnings` + `npm test` + `npm run
build` green before any task is done.

## 11. Observability checklist

- INFO: startup (binary path + source, accounts loaded, interval), gate
  transitions, cycle start/finish with per-account outcome + duration +
  trigger, env vars stripped (first poll per account).
- WARNING: skipped decisions with reason, timeout, spawn error, parse error,
  backoff entered, prune count, `.cmd` shim skipped.
- ERROR: guard tripped (with raw envelope), watchdog abort, DB errors.
- DEBUG: process-check timing and matched PIDs, raw result text, env vars
  stripped (subsequent polls).
- "Open log folder" in tray and settings; log level toggle at runtime.

## 12. Risks

| Risk | Mitigation |
|---|---|
| Claude Code changes `/usage` text | Fixture tests are the alarm; raw kept for every snapshot; status shows parse error instead of stale numbers |
| A flag in §2.2 is removed in a future CLI | Non-zero exit → `spawn_error` with stderr shown; flag set lives in one constant |
| The prompt reaches the model once (spends one turn) | Direct spawn, no shell, no batch shims, sanitised env; guard detects it and halts the whole poller (persisted) so it cannot repeat |
| Polling overhead on battery | Process gate; gap-based interval; serial polls; backoff |
| npm-installed Claude not detected by gate | Matcher covers `node` + package path; manual refresh always works; gate state visible in header |
| Orphaned child on quit | Explicit kill + wait in the `ExitRequested` hook (§6.5); `kill_on_drop` only as a fallback for abnormal task teardown |

## 13. Out of scope / deferred

- Any unofficial endpoint or claude.ai scraping.
- Per-account process attribution (which config dir a running claude uses).
- API-org usage/cost reporting (Admin API) — possible later add-on.
- v1.1 candidates: threshold notifications, configurable retention, parsing
  the "What's contributing" section, manual account reordering, npm-shim
  support on Windows, Windows job object for child cleanup, wake-from-sleep
  detection (immediate poll after resume), auto-update.

## 14. Verified-facts ledger

Facts an implementer may rely on without re-testing, with the date verified:

| Fact | Verified |
|---|---|
| Flag set in §2.2 returns `local_command: "usage"`, `num_turns: 0`, exit 0 | 2026-09-15 |
| `--no-session-persistence` prevents the session `.jsonl` write | 2026-09-15 |
| stdout is UTF-8, LF-only, no BOM | 2026-09-15 |
| Empty config dir → cost-summary text, exit 0 | 2026-09-15 |
| `~/.claude*` layouts in §2.3 | 2026-09-15 |
| Poll duration ~3 s with flag set, ~4 s without | 2026-09-15 |
| `--tools` accepting an empty value | not needed (flag dropped) |
| npm `cli.js` path fragment | **unverified** (no npm install here) |
| `app_log_dir()` Windows location | to be logged at startup; not load-bearing |

## 15. Review log

- 2026-09-15 v1: technical research (Tauri 2 APIs, plugin versions, sysinfo,
  rusqlite, tracing) by research agent; local verification by session.
- 2026-09-15 v1 → v2: hostile review (opus) 25 findings, consistency review
  (sonnet) 20 findings. All accepted except two redirected: minimum interval
  kept at 10 s but redefined as the inter-cycle gap (D5); raw pruning removed
  instead of restated (D8/D10). YAGNI cuts: drag-to-reorder, `bucket_secs`,
  `--tools ""`, `.cmd` shim, 50 ms process-check budget as a gate.
- 2026-09-15 v2 → v3: round 2. Hostile: all 25 round-1 items closed (the
  10 s redirect accepted), 1 new blocker (guard trip must halt the whole
  poller, not one account → global halt in §6.3), 8 should-fix (watchdog
  moved to driver, spawned cycle task + coalesced triggers, guard split into
  shape vs turn evidence, kill via handle not PID, sparkline gaps, deadline
  not restarted on settings change, `Halted` tray level, history split into
  its own command), 8 nits applied. Consistency: 19/19 closed, 2 must-fix
  (machine purity → `decide` returns descriptions; `NoEnabledAccounts`),
  2 should-fix (snake_case wire forms for all enums; events are refetch
  triggers only), 4 nits applied.
- 2026-09-15 v3 → v4: round 3. Hostile: all round-2 items closed; 4
  should-fix applied (guard order: `local_command` before shape checks +
  5-strike escalation; `clear_halt` no longer polls; halt persisted before
  cycle abort; `AccountChanged` accumulates ids); 10 nits applied. Verdict:
  **ready to plan, no further round needed.** Consistency: all round-2 items
  closed; 1 must-fix (`tray_state` takes `halted`) and 6 should-fix applied
  (855 MB worst case, `incremental_vacuum` in `prune`, settings change
  resets backoff, `stalled_at` home, no-enabled banner derivation,
  `clear_halt` test, advisory-rule fixture).
- 2026-09-16 v4 → v5 (final): round 4 consistency confirmation — all
  round-3 items closed, 0 must-fix, 3 should-fix applied (`local_command`
  ordering sentence made precise; escalation counter owned by `Backoff` in
  `machine.rs`; `DriverStatus` declared in §5.1 as the sole cross-task
  scheduler view) and 1 nit (backoff reset limited to polling-relevant
  settings). **Spec closed; implementation planning started.**
