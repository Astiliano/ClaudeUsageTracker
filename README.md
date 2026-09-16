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

Accounts can be reordered by dragging rows with the grip handle, or with the
Move up/Move down buttons inside a row's edit drawer. Column headers can also
be dragged to reorder; column order, typeface and text size persist per
machine in the webview's local storage. Click a row's sparkline to open the
30-day history drawer.

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

`src-tauri/Cargo.toml` builds two binaries (`claude-usage-tracker`, the app,
and `fake_claude`, a test double for the CLI). `default-run` picks
`claude-usage-tracker` so `cargo run` and the Tauri dev/build commands are
unambiguous without an explicit `--bin`. `fake_claude` is never bundled: the
Tauri bundle only packages `mainBinaryName` from `tauri.conf.json`
(`claude-usage-tracker`), and there is no `externalBin` list pulling
`fake_claude` in.

### Browser preview

```bash
# renders the UI with an in-memory mock backend, no Rust build needed
VITE_MOCK_BACKEND=1 npm run dev        # PowerShell: $env:VITE_MOCK_BACKEND='1'; npm run dev
```

The mock backend is never bundled into a production build: it's loaded via a
dynamic `import()` gated behind the `VITE_MOCK_BACKEND` env flag, so Vite only
pulls it into a dev build.

## Gates

All four must be green:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

Plain `cargo test` is the gate — do not add `--test-threads=1`. The runner and
driver integration tests each take a process-wide `ENV_LOCK` mutex for the
duration of the test (see `src-tauri/tests/runner_guard.rs` and
`src-tauri/tests/driver_loop.rs`), because they configure the `fake_claude`
helper through process-wide environment variables. That file-wide lock is
what serialises the tests that need it; the rest of the suite still runs
concurrently.

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

**Never run `/usage` through Git Bash.** Passing it through an MSYS shell
path-mangles the command into a real prompt and spends quota against your
subscription. Always invoke the CLI directly (as the app itself does) or from
a native `cmd`/PowerShell/native-terminal session, never from Git Bash.

**The Git Bash hazard is deliberately not reproduced as a test.** The app
spawns the binary directly precisely so this cannot happen; the envelope
guard is the detector for it, not a thing to test by triggering.

### Quota-safety notes

- **Safe flag set.** Every automated poll uses exactly:
  `-p /usage --output-format json --model haiku --no-session-persistence
  --strict-mcp-config --permission-prompts none --safe-mode`. This is the
  only argv the app ever spawns; it never persists a session, never accepts
  MCP servers from a project config, and never surfaces a permission prompt
  that could turn into a real, quota-spending turn.
- **The envelope guard and the halt.** Every reply is checked against the
  shape of a genuine `/usage` command output. If a reply ever looks like a
  model turn instead of the local `/usage` command reply, the guard trips:
  polling halts globally, the halt is persisted to the database immediately
  (before anything else happens), and it survives an app restart.
- **Clearing a halt.** A tripped halt shows as a red banner with a **Clear
  halt** button. Only clear it once you've confirmed the cause — for example
  a Claude Code upgrade that changed the `/usage` output shape — because
  polling stays off for every account until you do.

## Where things live

- Database: `<app data dir>/usage.sqlite` (30 days of snapshots, pruned daily)
- Logs: `<app log dir>/claude-usage-tracker.*.log` (7 daily files)
- Poll working directory: `<app data dir>/poll-cwd`
- Login helper script: `<app data dir>/login/` (emptied at startup)

The exact app data and log directories are logged at startup.
