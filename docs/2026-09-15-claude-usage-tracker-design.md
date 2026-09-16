# Claude Usage Tracker — Design Notes

Date: 2026-09-15
Status: planning (no code yet)

## Goal

A cross-platform desktop app that shows, task-manager style, the subscription
usage (5-hour session window, weekly window, weekly per-model window) for one
or more Claude accounts, refreshing live while Claude Code is in use, with
history so burn rate over the week is visible.

## What we found out (research)

| Data source | Covers | Official? | Verdict |
|---|---|---|---|
| `claude -p "/usage" --output-format json` | Subscription 5h / 7d / 7d-per-model %, reset times | Yes (documented `/usage` command, documented print mode) | **Use this** |
| Claude Code statusLine JSON `rate_limits` | Same numbers, structured | Yes | Only fires during a live session; not needed |
| Admin API usage/cost reports | API orgs only ("unavailable for individual accounts") | Yes | Not applicable to subscriptions |
| Enterprise Analytics API | Enterprise admins, aggregates | Yes | Not applicable |
| Internal endpoint Claude Code calls for `/usage`, claude.ai OAuth | Subscription | **No** | Rejected — unsupported, will break silently, ToS-gray |
| Scraping claude.ai settings in a webview | Subscription | No | Rejected — brittle |

Verified locally on 2026-09-15:

- `claude -p "/usage" --output-format json --model haiku` returns the usage
  text in the `result` field with `num_turns: 0` and `total_cost_usd: 0` —
  it is handled locally, no model call.
- It works per account by setting `CLAUDE_CONFIG_DIR` (tested `~/.claude`,
  `~/.claude2`, `~/.claude3`).
- Each call takes roughly 5–12 s (full Claude Code process start).
- From Git Bash the argument `/usage` gets MSYS path-mangled into
  `C:/Program Files/Git/usage`, which then reaches the model as a normal
  prompt and **spends a real turn** (one Fable turn cost the equivalent of
  $0.75). Spawn the binary directly, never through a shell; assert
  `num_turns == 0` on every response and treat anything else as a bug.
- Output shape (unversioned plain text):

  ```
  You are currently using your subscription to power your Claude Code usage

  Current session: 12% used · resets Sep 16, 3:30am (America/Los_Angeles)
  Current week (all models): 3% used · resets Sep 21, 8am (America/Los_Angeles)
  Current week (Fable): 4% used · resets Sep 21, 8am (America/Los_Angeles)

  What's contributing to your limits usage?
  ...
  ```

  `Current session: 0% used` appears with **no reset time** — reset is
  optional. The per-model line label ("Fable") varies by plan/model.

## Existing setup this builds on

Josh already runs multiple accounts as one Claude Code binary plus one config
dir per account, switched via `CLAUDE_CONFIG_DIR`:

- `~/.claude` (default), `~/.claude2`, `~/.claude3` — PowerShell functions
  `claude2` / `claude3` set the env var and call `~/.local/bin/claude.exe`.
- `~/.claude-free`, `~/.claude-kilofree`, `~/.claude-flow` also exist (other
  profiles; `/usage` on a non-subscription profile reports as such).

The app adopts this model directly: an "account" **is** a config dir. No
in-app OAuth. "Log in" opens a terminal with `CLAUDE_CONFIG_DIR` set running
`claude /login`, which is fully supported.

## Decisions made

1. **Data source:** `claude -p "/usage"` per account (above).
2. **Cross-platform:** yes. Stack: **Tauri 2** (Rust backend, React/TypeScript
   webview). Electron considered and rejected for size (~150 MB vs a few MB).
3. **Self-detect the default claude:**
   - Binary: `where`/`which claude`, fallback `~/.local/bin/claude[.exe]`;
     overridable in settings.
   - Default account: `CLAUDE_CONFIG_DIR` if set in the app's environment,
     else `~/.claude`.
   - Other accounts: auto-discover `~/.claude*` directories that look like
     Claude Code config dirs; label from dir name; user can rename, disable,
     remove, or add an arbitrary path.
4. **Refresh interval:** user-adjustable, **minimum 10 s**, default 60 s.
5. **Refresh gate — process-based, checked before every usage call:**
   - Before each cycle, check whether any `claude` process is running
     (`sysinfo` crate, cross-platform).
   - Running → poll all enabled accounts.
   - Transition running → not running (user stopped Claude) → **one final
     poll** of all accounts, then idle.
   - Not running → idle; only the (free) process check repeats on the
     interval. Manual "Refresh now" always works regardless of the gate.
   - Because polls are serialized (below), the app's own `claude -p` child is
     never alive at check time, so no self-exclusion logic is needed.
6. **Polling discipline:** one in-flight poll per account, accounts polled
   serially; if a tick arrives while a cycle is still running, skip it (never
   queue). UI shows "last updated N s ago" per account so slow cycles are
   visible.
7. **UI scope (option 3):**
   - Window: table with account, session %, week (all) %, week (model) %,
     reset countdowns, last-updated, status (ok / not subscription / parse
     error / spawn error / timeout).
   - System tray: icon, hover tooltip with worst account, close-to-tray,
     launch at login.
   - History: every poll persisted (SQLite); per-account sparkline / burn
     rate over the current week.
8. **Failure visibility, not silence:** a poll outcome is stored and shown as
   a status even when it fails ("parse error — output format changed?")
   rather than leaving stale numbers on screen. Raw text is kept with each
   snapshot so history can be re-parsed after a parser fix.
9. **Logging built in:** structured logs (`tracing`) to a rotating file in the
   app data dir; "Open log" menu item; DEBUG level toggle in settings.

## Proposed architecture (draft, not yet reviewed)

```
src-tauri/src/
  discovery.rs   find claude binary; enumerate ~/.claude* profiles
  process.rs     is_claude_running()
  usage/
    runner.rs    spawn claude -p "/usage" --output-format json --model haiku
                 with CLAUDE_CONFIG_DIR; 30 s timeout; assert num_turns == 0
    parser.rs    result text -> UsageSnapshot (pure, fixture-tested)
  scheduler.rs   tick loop: process gate, running->stopped final poll,
                 serial per-account polling, skip-if-busy
  store.rs       SQLite (rusqlite bundled): accounts, settings, snapshots
  commands.rs    Tauri commands: accounts CRUD, set interval, poll_now,
                 get_history, open_login
src/             React: table, sparklines, settings, tray wiring
```

Data flow: scheduler → runner → parser → store → emit `usage:updated` → UI.

Core types:

```rust
struct Account { id, label, config_dir, enabled }
struct Window  { pct: u8, resets_at: Option<DateTime<Utc>> }
struct UsageSnapshot {
  account_id, taken_at,
  session: Window,
  week_all: Window,
  week_model: Option<(String /* label, e.g. "Fable" */, Window)>,
  raw: String,
}
enum PollOutcome { Ok(UsageSnapshot), NotSubscription, ParseError(raw),
                   SpawnError(msg), Timeout }
```

## Testing intent

- Parser: fixture files for every observed output shape (with/without reset
  time, non-subscription profile, per-model line present/absent); a failing
  fixture is the alarm when Claude Code changes the format.
- Scheduler: state-machine tests for the process gate (idle → running →
  final-poll → idle), skip-if-busy, minimum interval clamp.
- Runner: `num_turns != 0` or `total_cost_usd != 0` is a hard failure with
  the raw response logged.
- Discovery: temp-dir fixtures for profile enumeration and binary lookup.

## Out of scope / rejected

- Any unofficial endpoint or claude.ai scraping.
- Per-account process attribution (which config dir a running claude uses)
  — not portable; global process gate chosen instead.
- API-org usage/cost reporting (different product; could be a later add-on
  via the Admin API for API-key profiles).

## Open items (not yet decided)

- Exact tray badge semantics (worst-of-all vs default account).
- History retention window (e.g. 30 days) and pruning.
- Whether to show the "What's contributing" breakdown section at all.
- Notification when an account crosses a threshold (e.g. 90% weekly).
