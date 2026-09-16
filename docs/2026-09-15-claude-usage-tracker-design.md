# Claude Usage Tracker — Design Spec

Date: 2026-09-15
Status: **spec v2 — under adversarial review** (see §15 Review log)

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
| D7 | One cycle at a time. Every poll path goes through the same `try_begin_cycle`; anything arriving while a cycle runs is **skipped, never queued** | Bounded resource use; one state machine |
| D8 | Every poll outcome is persisted and displayed; **raw text is stored with every snapshot** (success or failure) so any snapshot can be re-parsed after a parser fix | Failure visibility, not silence. Storage: ~1.1 KB/poll; at 60 s, 8 h/day, 3 accounts ≈ 1.6 MB/day ≈ 50 MB per retention window — acceptable |
| D9 | Tray icon reflects the **worst percentage across all enabled accounts and all their windows** (session, week-all, every per-model line); tooltip lists every enabled account | Worst-of is what you act on |
| D10 | History retention: snapshot rows older than **30 days** are deleted at startup and every 24 h. No separate raw pruning | Bounds DB size; D8 stays true for the whole window |
| D11 | The "What's contributing" section is **not parsed or displayed** in v1; it is preserved inside `raw` | YAGNI; unstable heuristics text |
| D12 | Threshold notifications: **deferred to v1.1** | Not core to the "task manager" value |
| D13 | Logging: `tracing` JSON lines to a daily-rotating file in the app log dir; runtime level toggle via `reload::Handle`; "Open log folder" menu item | Debuggable in production without adding code; firehose one switch away |
| D14 | Frontend testing: Vitest on pure helpers only. No E2E in v1 | Logic lives in Rust; the webview is presentational |
| D15 | Child environment is **sanitised**: inherit the parent environment, remove every variable whose name starts with `ANTHROPIC_` or `CLAUDE_`, then set `CLAUDE_CONFIG_DIR` | An inherited `ANTHROPIC_API_KEY` would turn a guard miss into metered spend; `CLAUDE_CODE_USE_BEDROCK` etc. would change the auth path |
| D16 | Per-account **failure backoff**: after N consecutive non-`Ok` outcomes the account is skipped for `min(15 min, 60 s × 2^(N-1))`; reset on `Ok`, manual refresh, account edit, or settings change | A logged-out account or missing binary must not write an identical failure row every cycle |
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
  tray.rs            tray_state(latest) -> (Level, tooltip)  [pure] + apply
  login.rs           open_terminal_for_login(binary, config_dir) per OS
  logging.rs         tracing init, reload handle
src/                 React: App, Header, AccountsTable, Sparkline, Settings,
                     FailureDetail, hooks/useDashboard
```

Data flow: `driver` → `runner` → `parser` → `store` → `emit("usage:updated")`
→ React refetches `get_dashboard` (debounced 250 ms). The frontend never
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
  Timeout,              // killed after timeout_secs; error = "timed out after {n}s"
  GuardTripped(String), // §6.3 violated — fatal for the account
}

pub struct Snapshot { id: i64, account_id: String, taken_at: i64 /*epoch ms*/,
                      outcome: PollOutcome, raw: Option<String>,
                      duration_ms: u32 }

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
- First start seeds the accounts table (default account enabled, others
  enabled too — they are the user's own profiles; not-logged-in ones will show
  `no_usage_data` and back off). `rescan_profiles` adds new candidates as
  **disabled** with `disabled_reason = User`.

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
  of the poll.
- Timeout `settings.timeout_secs` (default 30, min 5, max 120) via
  `tokio::time::timeout`; on expiry `kill().await`, then `wait().await`,
  return `Timeout`.
- **Envelope guard** (evaluated before parsing). All of the following must
  hold, else `GuardTripped(reason)`:
  1. stdout parses as JSON and `type == "result"`;
  2. `local_command` is **present and equals `"usage"`** (primary check — a
     model turn has no such field);
  3. `num_turns == 0` (secondary);
  4. `total_cost_usd == 0` (advisory: under subscription auth it can read 0
     for a billed turn, so it can only ever add a trip, never excuse one).
  On a trip: raw envelope logged at ERROR, the account is set
  `enabled=false, disabled_reason=GuardTripped`, outcome persisted, UI shows
  "guard tripped — polling disabled" until the user re-enables it.
- Non-zero exit → `SpawnError(stderr tail, 2 KB max)`. Exit 0 with
  non-JSON stdout → `SpawnError("non-JSON stdout: <tail>")`.
- Success returns `result` text + `duration_ms` to the parser.

### 6.4 `usage/parser.rs` (pure, fixture-tested)

- Input: result text, `now: DateTime<Utc>`.
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
  (month abbrev, day, 12-hour time with optional minutes). Zone = the IANA
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
pub enum Trigger { Timer, Manual, Startup, AccountChanged(AccountId) }
pub enum Decision { Run { accounts: Vec<AccountId>, reason: Trigger },
                    Skip(SkipReason) }
pub enum SkipReason { Busy, GateIdle, NoBinary, AllBackedOff }

pub fn decide(&mut self, t: Trigger, claude_running: Option<bool>,
              binary_present: bool, enabled: &[AccountId], now: i64) -> Decision
```

Rules, evaluated in this order:

1. `cycle.is_some()` → `Skip(Busy)`. (Busy is checked **before** the process
   check, so the app's own child can never latch the gate.)
2. `!binary_present` → `Skip(NoBinary)` for every trigger.
3. `Timer`: `claude_running` must be `Some`. Idle&running → gate=Active, Run;
   Active&running → Run; Active&!running → gate=Idle, Run (the one final
   poll); Idle&!running → `Skip(GateIdle)`.
   `Manual`/`Startup`: Run all enabled, gate untouched.
   `AccountChanged(id)`: Run `[id]` only, gate untouched.
4. Filter the run list by backoff (D16): drop accounts whose
   `next_allowed > now` unless trigger is `Manual` or `AccountChanged`
   (both reset that account's backoff). Empty list → `Skip(AllBackedOff)`.
5. Gate transitions emit `gate:changed`.

`begin_cycle() -> CycleToken` sets `cycle`; the token is an RAII guard whose
`Drop` calls `end_cycle()`, so a panic or task abort can never leave the
machine busy. `record(account, outcome, now)` updates backoff.
`watchdog(now)`: if a cycle has been open longer than
`enabled.len() × timeout_secs + 10 s`, force-clear it, kill any child PID
still registered, log ERROR, and emit `poller:stalled {at, cycle_age_ms}`;
the header shows "Poller stalled at HH:MM — recovered (see log)" until the
next successful cycle. A stuck **child** never reaches the watchdog: the
per-poll timeout kills it and records `timeout` on that account's row.

#### `driver.rs`

- Startup: `decide(Startup)` → run. Then loop:
  `select! { _ = sleep(gap) => Timer, _ = settings_rx.changed() => rebuild gap,
            Some(t) = trigger_rx.recv() => t, _ = shutdown.cancelled() => break }`
  where `gap` = `interval_secs` measured from the **end of the last cycle**.
  Interval changes therefore apply immediately (the sleep is rebuilt).
- Before a `Timer` decision the driver runs the process check with
  `exclude_pid` = current child PID (always `None` here because busy was
  already checked, kept as belt-and-braces).
- Binary presence is re-checked (`stat`) before every decision, so a
  first-run "binary not found" state clears as soon as the user fixes
  Settings.
- A cycle polls accounts serially in D17 order; after each account the
  outcome is persisted, backoff recorded, `usage:updated {account_id}` and
  the tray refreshed; `cycle:finished` at the end.
- Shutdown (tray Quit or window close when not close-to-tray): cancel the
  token; the in-flight child is killed by `kill_on_drop`; wait ≤ 2 s for the
  cycle task, then exit.
- Sleep/wake: nothing special; the next `Timer` after wake simply runs late.

### 6.6 `store/`

SQLite via `rusqlite` (bundled), file `<app_data_dir>/usage.sqlite`. Open
with `journal_mode=WAL`, `foreign_keys=ON`, `busy_timeout=5000`. A single
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
`history(account_id, since)` → hourly buckets of `max(week_all_pct)` over
`ok` rows, `prune(now)` deletes rows with `taken_at < now − 30 d` and logs
the count at WARN if > 0.

Settings keys: `interval_secs`, `timeout_secs`, `claude_binary` (override
path or empty), `close_to_tray` (default true), `log_level`
(`info`|`debug`). `launch_at_login` is **not** stored: `get_settings` reads
the autostart plugin's live state and `set_settings` writes through to it.
`set_settings` also applies `log_level` via the reload handle immediately.

### 6.7 Tray (`tray.rs`)

- Menu: Open (default item), Refresh now, Open log folder, Quit. Left-click
  shows + focuses the window where the platform delivers click events
  (Windows, macOS); on Linux AppIndicator the default menu item is the path.
- Window close → hide when `close_to_tray`, else quit.
- `tray_state(latest: &[(Account, Option<SnapshotDto>)]) -> (Level, String)`
  is pure and unit-tested. `Level` ∈ `Grey` (no enabled account has an `ok`
  snapshot), `Green` (< 70), `Amber` (70–89), `Red` (≥ 90) from the max pct
  across every window of every enabled account's latest `ok` snapshot.
  Tooltip: one line per enabled account, `label  S 15% · W 4% · Fable 5%`
  (per-model segments repeated by label, omitted when none; `err` when the
  latest outcome is not `ok`).
- Applied after each account's poll and on account/settings changes.

### 6.8 Login (`login.rs`)

Opens a visible terminal running the discovered binary with `/login` and
`CLAUDE_CONFIG_DIR` set:
- Windows: `cmd.exe /k` with `set "CLAUDE_CONFIG_DIR=<dir>" && "<binary>" /login`
  (no CREATE_NO_WINDOW; `/login` is safe here because no MSYS is involved
  and the worst case is an interactive prompt the user sees).
- macOS: write `<app_data_dir>/login-<id>.command` (`#!/bin/bash`,
  `export CLAUDE_CONFIG_DIR=…; exec "<binary>" /login`), `chmod 700`,
  `open` it.
- Linux: first available of `gnome-terminal -- <script>`, `konsole -e
  <script>`, `xfce4-terminal -e <script>`, `xterm -e <script>` using the same
  script-file approach.
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

- **Header**: gate/binary state banner — one of "Claude running — polling
  every 60 s", "Idle — will resume when Claude Code starts", "Claude binary
  not found — set it in Settings" (with a button), "Polling paused: no
  enabled accounts". Refresh now, Settings.
- **Accounts table** (D17 order): label · Session % bar + "resets in 3h 12m"
  · Week (all) % · one cell per per-model line ("Fable 5%"; "—" if none) ·
  sparkline (hourly max week-all pct, last 7 days) · last updated ("42 s
  ago", live) · status pill. Pill precedence: `disabled` (with reason
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
| `get_dashboard` | → `{ accounts: [{ account: Account, latest: SnapshotDto?, backoff_until: i64?, history: [{t, pct}] }], gate, busy, binary: { path?, source? }, interval_secs }` |
| `poll_now` | → `"started" \| "skipped:<reason>"` |
| `add_account` | `{config_dir}` → Account. Canonicalises; rejects missing dir (`not_found`) or duplicate (`duplicate`) |
| `update_account` | `{id, label?, enabled?}` → Account. Enabling clears `disabled_reason` and triggers `AccountChanged` |
| `remove_account` | `{id}` → () (cascades snapshots) |
| `rescan_profiles` | → `[Account]` newly added (disabled) |
| `get_settings` / `set_settings` | full settings struct; clamps rejected with `out_of_range`; change triggers settings watch |
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
| runner | Envelope guard table (each rule violated in turn, incl. **missing `local_command`** → `guard_tripped`); non-zero exit → `spawn_error`; exit 0 + non-JSON → `spawn_error`; timeout via a fake slow binary (`tests/fixtures/slow_claude.rs` built as a test helper binary) → `timeout` and the child is gone; exact argv constant; env sanitisation (ANTHROPIC_/CLAUDE_ vars removed, `CLAUDE_CONFIG_DIR` set) using a fake binary that echoes its env |
| scheduler/machine | Decision table for `Timer` (4 gate×running combos), busy precedes process check, `NoBinary` for every trigger, `Manual` and `Startup` ignore gate, `AccountChanged` runs one account, final-poll-once property (running→stopped→stopped = exactly one Run), backoff schedule and reset rules, RAII token clears busy on drop (including panic via `catch_unwind`), watchdog force-clear |
| discovery | Temp-home fixtures reproducing §2.3 (incl. exclusion of the `update-state.json`-only dir); label derivation; D17 ordering; binary precedence with a fake PATH; `.cmd` rejected on Windows; canonicalisation |
| store | Migrations from empty; latest-per-account with same-ms tiebreak; hourly history buckets; prune at the 30 d boundary; **cascade on account removal (proves `foreign_keys=ON`)**; unique on canonical path |
| process | `matches_claude` on synthetic (name, cmd) tuples: native, npm, unrelated node, excluded pid |
| tray | `tray_state`: grey/green/amber/red thresholds, multi-account worst-of, per-model segments, `err` rendering |
| commands | `add_account` rejects missing/duplicate; `set_settings` boundary values (10/3600, 5/120) accept, one-off values reject; `update_account` enable clears `disabled_reason` |
| frontend | Vitest: countdown formatter, "N s ago" formatter, sparkline path builder, status pill precedence |
| integration (manual, documented in README) | Real poll against `~/.claude3`; Git Bash hazard reproduction is **not** run (costs quota) |

Gate: `cargo test` + `cargo clippy -D warnings` + `npm test` + `npm run
build` green before any task is done.

## 11. Observability checklist

- INFO: startup (binary path + source, accounts loaded, interval), gate
  transitions, cycle start/finish with per-account outcome + duration +
  trigger, env vars stripped (first poll per account).
- WARNING: skipped decisions with reason, timeout, spawn error, parse error,
  backoff entered, prune count, `.cmd` shim skipped.
- ERROR: guard tripped (with raw envelope), watchdog force-clear, DB errors.
- DEBUG: process-check timing and matched PIDs, raw result text, env vars
  stripped (subsequent polls).
- "Open log folder" in tray and settings; log level toggle at runtime.

## 12. Risks

| Risk | Mitigation |
|---|---|
| Claude Code changes `/usage` text | Fixture tests are the alarm; raw kept for every snapshot; status shows parse error instead of stale numbers |
| A flag in §2.2 is removed in a future CLI | Non-zero exit → `spawn_error` with stderr shown; flag set lives in one constant |
| The prompt reaches the model once (spends one turn) | Direct spawn, no shell, no batch shims, sanitised env; guard detects it and disables the account so it cannot repeat |
| Polling overhead on battery | Process gate; gap-based interval; serial polls; backoff |
| npm-installed Claude not detected by gate | Matcher covers `node` + package path; manual refresh always works; gate state visible in header |
| Orphaned child on quit | `kill_on_drop` + shutdown hook |

## 13. Out of scope / deferred

- Any unofficial endpoint or claude.ai scraping.
- Per-account process attribution (which config dir a running claude uses).
- API-org usage/cost reporting (Admin API) — possible later add-on.
- v1.1 candidates: threshold notifications, configurable retention, parsing
  the "What's contributing" section, manual account reordering, npm-shim
  support on Windows, Windows job object for child cleanup, auto-update.

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
- Round 2: pending.
