# History Range, Hideable Columns, Compact View — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the history drawer range/granularity/metric controls backed by a server-side bucketing query with hard limits, let the user hide table columns, add a card-based compact layout with ring gauges and an always-on-top toggle, and remove the default-account flag end to end.

**Architecture:** The Rust store gains one bucketing query parameterised by `since`, `bucket_ms` and a metric enum (week-all, session, or a model label via `json_each`), plus a distinct-labels query; the command layer validates limits and rejects with `out_of_range`. On the frontend every rule lives in a pure, Vitest-covered module under `src/lib/` (`history.ts`, `columns.ts`, `prefs.ts`, `layout.ts`, `gauge.ts`); React components render those decisions. The drawer owns its own fetch; the dashboard hook keeps feeding the 7-day sparkline. A shared `historyLimits.json` keeps the Rust and TypeScript limits identical.

**Tech Stack:** React 19, TypeScript 6 (strict, `noUnusedLocals`, `noUnusedParameters`, `resolveJsonModule`), Vite 8, Vitest 5 (node environment, `src/**/*.test.ts` only, no jsdom), Tauri 2 (`@tauri-apps/api` 2.11), Rust with rusqlite 0.40 (bundled SQLite 3.53.2), serde, tracing.

**Spec:** `docs/superpowers/specs/2026-09-16-history-and-compact-design.md` — every task below argues from it; read it first.

## Global Constraints

- Four gates, all green at the end of every task: `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`, `npm test`, `npm run build`. Plain `cargo test`; never add `--test-threads=1`. Every task in this plan is written so all four gates pass at its end, on its own.
- TypeScript: no `any`, no non-null `!` assertions, no silent catches (every `catch` either surfaces via `onError` or `console.warn`s with context). `noUnusedLocals` / `noUnusedParameters` are on, and test files are type-checked by `npm run build` (`tsc`), so a test with an unused import fails the build gate.
- Vitest runs in the **node** environment: tests may only import pure modules (`src/lib/*`). No React rendering in tests. No `node:fs` (`@types/node` is not installed).
- `import type { JSX } from "react"` for component return types (existing convention).
- Every backend call in React code goes through a try/catch that routes failure to `onError(message)` (or `setError` inside `useDashboard`), using `errorMessage(e)` from `src/lib/errors.ts`.
- No chart library; the chart stays hand-rolled SVG. Missing buckets are breaks, never zeros.
- Dark theme only. CSP is `default-src 'self'; style-src 'self' 'unsafe-inline'`; no external URLs.
- Every timer, `window` listener, `ResizeObserver` and `listen()` subscription is removed on unmount.
- No new npm dependencies.
- Do not create or enter git worktrees; work in the session checkout on branch `history-and-compact`.
- Text size is CSS `zoom` on `main.app`; anything measured in viewport px goes through `toLocal(px, zoom)` from `src/lib/drag.ts`.
- Commits: one per task, message prefix as given in each task, ending with the attribution trailer the session provides (`Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and the `Claude-Session:` line).
- Browser preview: `VITE_MOCK_BACKEND=1 npm run dev` (PowerShell: `$env:VITE_MOCK_BACKEND='1'; npm run dev`) serves the UI at `http://localhost:1420` against `src/lib/mockBackend.ts`. A running release build of the app holds a single-instance lock: stop it before `npm run tauri dev`.
- Decisions already made (do not re-open): metric set week-all + session + every model label; presets/limits per spec §3.2; breakpoints 820 / 640 local px, min window 360×240; ring gauges; always-on-top as a Settings toggle in prefs; default flag removed end to end; nothing hidden by default; the hint line under the table is removed.

## File structure

Create:
- `src/lib/historyLimits.json` — the three shared limits.
- `src/lib/history.ts` + `src/lib/history.test.ts` — presets, units, alignment, bucketing, labels, hover slot, metric helpers.
- `src/lib/layout.ts` + `src/lib/layout.test.ts` — breakpoints, `layoutFor`, `autoHiddenColumns`, `SHELL_PADDING`.
- `src/lib/gauge.ts` + `src/lib/gauge.test.ts` — ring geometry.
- `src/hooks/useViewport.ts` — `window.innerWidth` via `ResizeObserver`.
- `src/components/StatusPill.tsx` — moved out of `AccountRow.tsx` so cards can reuse it.
- `src/components/Ring.tsx`, `src/components/AccountCard.tsx`.

Modify:
- Rust: `src-tauri/src/store/schema.rs`, `store/accounts.rs`, `store/snapshots.rs`, `store/mod.rs`, `commands.rs`, `lib.rs`, `paths.rs`, `tray.rs`, `usage/mod.rs`, `scheduler/driver.rs`; `src-tauri/tauri.conf.json`; `src-tauri/capabilities/default.json`.
- TypeScript: `src/lib/types.ts`, `series.ts` (+test), `columns.ts` (+test), `prefs.ts` (+test), `present.ts` (+test), `backend.ts`, `mockBackend.ts`; `src/hooks/useDashboard.ts`; `src/components/HistoryDrawer.tsx`, `AccountRow.tsx`, `AccountsTable.tsx`, `Settings.tsx`, `Header.tsx`; `src/App.tsx`; `src/styles.css`; any `src/lib/*.test.ts` that builds an `Account` literal.
- Docs: `docs/2026-09-15-claude-usage-tracker-design.md`, `README.md`.

---

### Task 1: Remove the default-account flag (Rust) and add the V3 migration

**Files:**
- Modify: `src-tauri/src/store/schema.rs`, `src-tauri/src/store/accounts.rs`, `src-tauri/src/usage/mod.rs`, `src-tauri/src/tray.rs` (test helper only), `src-tauri/src/commands.rs` (`core_add_account`), `src-tauri/src/scheduler/driver.rs` (test helper only), `src-tauri/src/store/snapshots.rs` (test helpers only), `src-tauri/src/lib.rs`, `src-tauri/src/paths.rs`
- Test: same files' `mod tests`

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `Store::add_account(&self, config_dir: &Path, enabled: bool, disabled_reason: Option<DisabledReason>, now: i64) -> AppResult<Account>` (the `is_default` parameter is gone).
  - `Store::seed_accounts_if_empty(&self, candidates: &[Candidate], now: i64) -> AppResult<usize>` (the `default_dir` parameter is gone; candidates are seeded in case-insensitive label order).
  - `Account` has no `is_default` field. `SCHEMA_VERSION = 3`.

- [ ] **Step 1: Write the failing schema tests**

In `src-tauri/src/store/schema.rs`, inside `mod tests`, add:

```rust
    fn column_names(conn: &rusqlite::Connection) -> Vec<String> {
        let mut stmt = conn.prepare("PRAGMA table_info(accounts)").expect("prepare");
        stmt.query_map([], |r| r.get::<_, String>(1))
            .expect("query")
            .collect::<Result<Vec<String>, rusqlite::Error>>()
            .expect("collect")
    }

    #[test]
    fn a_fresh_database_has_no_is_default_column_and_can_insert_an_account() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let names = store.with_conn(|c| Ok(column_names(c))).expect("table_info");
        assert!(!names.iter().any(|n| n == "is_default"), "V3 must drop is_default: {names:?}");
        assert!(names.iter().any(|n| n == "sort_order"));

        let dir = tmp.path().join(".claude3");
        std::fs::create_dir_all(&dir).expect("mkdir");
        store.add_account(&dir, true, None, 1).expect("insert must work without is_default");
    }

    #[test]
    fn migrating_from_v1_drops_is_default_and_keeps_rows_and_sort_order() {
        let mut conn = rusqlite::Connection::open_in_memory().expect("open");
        apply_pragmas(&conn).expect("pragmas");
        conn.execute_batch(V1).expect("create v1 schema by hand");
        conn.execute_batch("PRAGMA user_version=1;").expect("set v1");
        conn.execute(
            "INSERT INTO accounts(id, label, config_dir, enabled, disabled_reason, is_default, created_at)
             VALUES ('bravo', 'Bravo', '/bravo', 1, NULL, 0, 100)",
            [],
        )
        .expect("insert bravo");
        conn.execute(
            "INSERT INTO accounts(id, label, config_dir, enabled, disabled_reason, is_default, created_at)
             VALUES ('alpha', 'Alpha', '/alpha', 1, NULL, 1, 50)",
            [],
        )
        .expect("insert alpha");

        migrate(&mut conn).expect("migrate v1 -> v3");

        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).expect("version");
        assert_eq!(version, 3);
        assert!(!column_names(&conn).iter().any(|n| n == "is_default"));

        let ordered: Vec<(String, i64)> = conn
            .prepare("SELECT id, sort_order FROM accounts ORDER BY sort_order ASC")
            .expect("prepare")
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("query")
            .collect::<Result<Vec<(String, i64)>, rusqlite::Error>>()
            .expect("collect");
        // The V2 backfill ran while is_default still existed, so alpha (the
        // old default) keeps sort_order 0 after V3 drops the column.
        assert_eq!(ordered, vec![("alpha".to_string(), 0), ("bravo".to_string(), 1)]);
    }

    #[test]
    fn each_migration_step_bumps_to_its_own_literal_version() {
        // A fresh DB must pass through 1, 2 and 3 in turn; if V2 jumped straight
        // to SCHEMA_VERSION the V3 step would be skipped (the bug the spec §6 names).
        let mut conn = rusqlite::Connection::open_in_memory().expect("open");
        apply_pragmas(&conn).expect("pragmas");
        conn.execute_batch(V1).expect("v1");
        conn.execute_batch("PRAGMA user_version=1;").expect("set v1");
        conn.execute_batch(V2_ALTER).expect("v2 alter");
        conn.execute_batch(V2_BACKFILL).expect("v2 backfill");
        conn.execute_batch("PRAGMA user_version=2;").expect("set v2");
        migrate(&mut conn).expect("migrate v2 -> v3");
        assert!(!column_names(&conn).iter().any(|n| n == "is_default"));
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).expect("version");
        assert_eq!(version, 3);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml schema::tests`
Expected: compile error (`add_account` takes 5 arguments; `is_default` column still present) — the failures are the point.

- [ ] **Step 3: Implement the schema change**

In `src-tauri/src/store/schema.rs`:

```rust
pub const SCHEMA_VERSION: i64 = 3;
```

After `V2_BACKFILL` add:

```rust
/// V3 (2026-09-16): the default-account flag is gone. `sort_order` has been
/// authoritative since V2 and nothing read `is_default` any more.
const V3_DROP: &str = "ALTER TABLE accounts DROP COLUMN is_default;";
```

Replace the body of `migrate` with:

```rust
pub fn migrate(conn: &mut Connection) -> AppResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current < 1 {
        let tx = conn.transaction()?;
        tx.execute_batch(V1)?;
        tx.execute_batch("PRAGMA user_version=1;")?;
        tx.commit()?;
    }

    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current < 2 {
        let tx = conn.transaction()?;
        tx.execute_batch(V2_ALTER)?;
        tx.execute_batch(V2_BACKFILL)?;
        // Each step bumps to its OWN literal version. Bumping to
        // SCHEMA_VERSION here would skip every later step on a fresh DB.
        tx.execute_batch("PRAGMA user_version=2;")?;
        tx.commit()?;
    }

    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current < 3 {
        let tx = conn.transaction()?;
        tx.execute_batch(V3_DROP)?;
        tx.execute_batch("PRAGMA user_version=3;")?;
        tx.commit()?;
    }

    Ok(())
}
```

The existing test `migrating_from_a_v1_database_backfills_sort_order_in_d17_order_and_is_idempotent` keeps passing unchanged (it asserts `SCHEMA_VERSION` and the alpha/bravo order, both still true).

- [ ] **Step 4: Remove `is_default` from the `Account` struct**

In `src-tauri/src/usage/mod.rs`, the struct becomes:

```rust
/// An account is a Claude Code config directory (D4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Account {
    pub id: String,
    pub label: String,
    pub config_dir: PathBuf,
    pub enabled: bool,
    pub disabled_reason: Option<DisabledReason>,
    pub created_at: i64,
    /// Manual order (D17, revised 2026-09-16): lower sorts first. Authoritative
    /// for `list_accounts`/`enabled_account_ids`; label is the only tiebreak.
    pub sort_order: i64,
}
```

In its test `account_serialises_config_dir_as_a_string`, delete the `is_default: false,` line from the literal and the `assert_eq!(v["is_default"], false);` line.

- [ ] **Step 5: Rewrite the accounts store**

In `src-tauri/src/store/accounts.rs`:

```rust
/// D17 (revised 2026-09-16): `sort_order` is authoritative; label is the
/// only tiebreak (relevant only while rows share a `sort_order`, which
/// normal use never produces once every row has gone through an insert or
/// `reorder_accounts`).
const ORDER_ACCOUNTS: &str = "ORDER BY sort_order ASC, lower(label) ASC, label ASC";

fn row_to_account(row: &Row<'_>) -> Result<Account, rusqlite::Error> {
    let reason: Option<String> = row.get("disabled_reason")?;
    Ok(Account {
        id: row.get("id")?,
        label: row.get("label")?,
        config_dir: PathBuf::from(row.get::<_, String>("config_dir")?),
        enabled: row.get::<_, i64>("enabled")? != 0,
        disabled_reason: reason.as_deref().and_then(DisabledReason::from_wire),
        created_at: row.get("created_at")?,
        sort_order: row.get("sort_order")?,
    })
}

const SELECT_COLS: &str =
    "id, label, config_dir, enabled, disabled_reason, created_at, sort_order";
```

`build_account` loses its `is_default` parameter and field:

```rust
fn build_account(
    config_dir: &Path,
    enabled: bool,
    disabled_reason: Option<DisabledReason>,
    sort_order: i64,
    now: i64,
) -> AppResult<(Account, String)> {
    if !config_dir.is_dir() {
        return Err(AppError::NotFound(format!(
            "no such directory: {}",
            config_dir.display()
        )));
    }
    let canonical =
        dunce::canonicalize(config_dir).unwrap_or_else(|_| config_dir.to_path_buf());
    let canonical_str = canonical.to_string_lossy().to_string();
    let account = Account {
        id: uuid::Uuid::new_v4().to_string(),
        label: label_for(&canonical),
        config_dir: canonical,
        enabled,
        disabled_reason,
        created_at: now,
        sort_order,
    };
    Ok((account, canonical_str))
}
```

`insert_account`'s statement becomes:

```rust
    conn.execute(
        "INSERT INTO accounts(id, label, config_dir, enabled, disabled_reason, created_at, sort_order)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            account.id,
            account.label,
            canonical_str,
            i64::from(account.enabled),
            account.disabled_reason.map(|r| r.as_str()),
            account.created_at,
            account.sort_order
        ],
    )
    .map_err(|e| map_unique_violation(e, canonical_str))?;
```

`add_account`:

```rust
    pub fn add_account(
        &self,
        config_dir: &Path,
        enabled: bool,
        disabled_reason: Option<DisabledReason>,
        now: i64,
    ) -> AppResult<Account> {
        self.with_conn(|c| {
            let sort_order = next_sort_order(c)?;
            let (account, canonical_str) =
                build_account(config_dir, enabled, disabled_reason, sort_order, now)?;
            insert_account(c, &account, &canonical_str)?;
            Ok(account)
        })
    }
```

`seed_accounts_if_empty`:

```rust
    /// Spec 6.1: first start seeds every candidate **enabled**, in
    /// case-insensitive label order. The emptiness check and every insert
    /// run inside one `with_conn` closure (one `Mutex<Connection>` hold) so
    /// a concurrent `add_account` or `seed_accounts_if_empty` call can't
    /// interleave and double-seed.
    pub fn seed_accounts_if_empty(&self, candidates: &[Candidate], now: i64) -> AppResult<usize> {
        self.with_conn(|c| {
            let existing: i64 =
                c.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
            if existing > 0 {
                return Ok(0);
            }

            let mut built: Vec<(Account, String)> = Vec::with_capacity(candidates.len());
            for cand in candidates {
                built.push(build_account(&cand.config_dir, true, None, 0, now)?);
            }
            built.sort_by(|(a, _), (b, _)| {
                a.label
                    .to_lowercase()
                    .cmp(&b.label.to_lowercase())
                    .then_with(|| a.label.cmp(&b.label))
            });

            let mut added = 0usize;
            for (i, (mut account, canonical_str)) in built.into_iter().enumerate() {
                account.sort_order = i as i64;
                match insert_account(c, &account, &canonical_str) {
                    Ok(()) => added += 1,
                    Err(AppError::Duplicate(_)) => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(added)
        })
    }
```

In `rescan_accounts` the call becomes `self.add_account(&c.config_dir, false, Some(DisabledReason::User), now)`.

Tests in this file: every `add_account(&x, a, b, <bool>, NOW)` call drops the fourth argument. Delete every `assert!(a.is_default)` / `assert!(!a.is_default)` line. Rename and rewrite two tests:

```rust
    #[test]
    fn add_account_appends_in_call_order_regardless_of_label() {
        // D17 revised 2026-09-16: sort_order is authoritative once accounts
        // exist, so add_account (which only ever appends) never resorts by
        // label -- that would silently fight any manual order the user set.
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let zed = make_dir(tmp.path(), ".claudeZed");
        let alpha = make_dir(tmp.path(), ".claudealpha");
        let main = make_dir(tmp.path(), ".claudeMain");

        store.add_account(&zed, true, None, NOW).expect("a");
        store.add_account(&alpha, true, None, NOW).expect("b");
        store.add_account(&main, true, None, NOW).expect("c");

        let labels: Vec<String> = store
            .list_accounts()
            .expect("list")
            .into_iter()
            .map(|a| a.label)
            .collect();
        assert_eq!(labels, vec!["claudeZed", "claudealpha", "claudeMain"]);
    }

    #[test]
    fn seeding_orders_candidates_by_case_insensitive_label() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let zzz = make_dir(tmp.path(), ".claudeZzz");
        let aaa = make_dir(tmp.path(), ".claudeaaa");

        store
            .seed_accounts_if_empty(
                &[candidate(&zzz, "claudeZzz"), candidate(&aaa, "claudeaaa")],
                NOW,
            )
            .expect("seed");

        let accounts = store.list_accounts().expect("list");
        assert_eq!(accounts[0].label, "claudeaaa");
        assert_eq!(accounts[0].sort_order, 0);
        assert_eq!(accounts[1].label, "claudeZzz");
        assert_eq!(accounts[1].sort_order, 1);
    }
```

`seeding_an_empty_store_enables_every_candidate_and_flags_the_default` becomes `seeding_an_empty_store_enables_every_candidate`: drop the `&one,` argument and the two `is_default` asserts, keep `assert_eq!(all[0].label, "claude");`. `seeding_is_skipped_when_accounts_already_exist` and `rescan_adds_only_new_candidates_and_adds_them_disabled` drop the `&one,` seed argument / the `true,` add argument. `enabled_account_ids_skips_disabled_rows_and_keeps_d17_order`: drop the boolean from its three `add_account` calls; its expected order is call order, which is unchanged.

- [ ] **Step 6: Fix the other callers**

- `src-tauri/src/commands.rs` (`core_add_account`): `let account = core.store.add_account(config_dir, true, None, now)?;`
- `src-tauri/src/scheduler/driver.rs` test helper: `.add_account(&dir, true, None, 1)`
- `src-tauri/src/store/snapshots.rs` tests: `.add_account(&dir, true, None, NOW)` and `.add_account(&d, true, None, NOW)`
- `src-tauri/src/tray.rs` test helper `account()`: delete the `is_default: false,` line.
- `src-tauri/src/lib.rs`: replace the seed block with

```rust
            // Seed accounts on first start (spec 6.1).
            let home = paths::home_dir()?;
            let candidates = discovery::enumerate_profiles(&home);
            let seeded = store.seed_accounts_if_empty(
                &candidates,
                chrono::Utc::now().timestamp_millis(),
            )?;
            info!(seeded, discovered = candidates.len(), "accounts loaded");
```

- `src-tauri/src/paths.rs`: delete `default_config_dir` and its doc comment, and the three tests `default_config_dir_prefers_the_env_override`, `default_config_dir_falls_back_to_dot_claude`, `blank_env_override_is_treated_as_unset`; fix the test `use` line to `use super::{db_path, empty_dir, ensure_dir, login_script_dir, poll_cwd};`.

Then `grep -rn is_default src-tauri/src` must return only `schema.rs` (V1 DDL, V2 backfill, V3 drop, and the migration tests' hand-written V1 inserts).

- [ ] **Step 7: Run all four gates**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`, `npm test`, `npm run build`
Expected: all green (the frontend still has its own `is_default`; the wire simply stops carrying it, which TypeScript cannot see — Task 3 removes it).

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src
git commit -m "refactor(store): drop the default-account flag; V3 migration removes the column"
```

---

### Task 2: Shared limits, `HistoryMetric`, bucketing queries and the commands

**Files:**
- Create: `src/lib/historyLimits.json`
- Modify: `src-tauri/src/store/snapshots.rs`, `src-tauri/src/store/mod.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`
- Test: `src-tauri/src/store/snapshots.rs` and `src-tauri/src/commands.rs` `mod tests`

**Interfaces:**
- Produces (Rust, re-exported from `crate::store`):
  - `pub const MIN_BUCKET_MS: i64 = 60_000; pub const MAX_BUCKETS: i64 = 1_000; pub const MAX_RANGE_MS: i64 = 2_592_000_000; pub const RANGE_SLACK_MS: i64 = 300_000; pub const MAX_LABEL_LEN: usize = 64;`
  - `pub enum HistoryMetric { WeekAll, Session, Model { label: String } }` — serde internally tagged on `kind`, snake_case: `{"kind":"week_all"}`, `{"kind":"session"}`, `{"kind":"model","label":"Fable"}`.
  - `Store::history(&self, account_id: &str, since: i64, bucket_ms: i64, metric: &HistoryMetric) -> AppResult<Vec<HistoryPoint>>` — buckets anchored at `since`, value = max, empty buckets absent, only `outcome = 'ok'` rows.
  - `Store::history_models(&self, account_id: &str, since: i64) -> AppResult<Vec<String>>` — distinct, sorted labels (non-blank, ≤ 64 chars).
  - `pub fn core_get_history(core: &Core, account_id: &str, now: i64, since: i64, bucket_ms: i64, metric: &HistoryMetric) -> AppResult<Vec<HistoryPoint>>` and `pub fn core_get_history_models(core: &Core, account_id: &str, now: i64) -> AppResult<Vec<String>>`.
  - Tauri commands `get_history { accountId, since, bucketMs, metric }` and `get_history_models { accountId }` (Tauri maps camelCase args to the snake_case parameters, as `get_history` already relies on today).
- Produces (shared): `src/lib/historyLimits.json` = `{"minBucketMs": 60000, "maxBuckets": 1000, "maxRangeMs": 2592000000}`.

- [ ] **Step 1: Create the shared limits file**

`src/lib/historyLimits.json`:

```json
{
  "minBucketMs": 60000,
  "maxBuckets": 1000,
  "maxRangeMs": 2592000000
}
```

- [ ] **Step 2: Write the failing store tests**

In `src-tauri/src/store/snapshots.rs` `mod tests`, replace the three existing `history_*` tests with these (the helper `ok_outcome` stays; add `ok_with_models`):

```rust
    fn ok_with_models(session: u8, week: u8, models: &[(&str, u8)]) -> PollOutcome {
        PollOutcome::Ok(Parsed {
            session: Window { pct: session, resets_at: None },
            week_all: Window { pct: week, resets_at: None },
            week_models: models
                .iter()
                .map(|(l, p)| ((*l).to_string(), Window { pct: *p, resets_at: None }))
                .collect(),
        })
    }

    #[test]
    fn limits_match_the_shared_json_the_ui_reads() {
        let raw = include_str!("../../../src/lib/historyLimits.json");
        let v: serde_json::Value = serde_json::from_str(raw).expect("valid json");
        assert_eq!(v["minBucketMs"], MIN_BUCKET_MS);
        assert_eq!(v["maxBuckets"], MAX_BUCKETS);
        assert_eq!(v["maxRangeMs"], MAX_RANGE_MS);
        assert_eq!(MAX_RANGE_MS, RETENTION_MS, "the chart can reach exactly as far as retention");
    }

    #[test]
    fn history_metric_deserialises_from_the_tagged_wire_shape() {
        let w: HistoryMetric = serde_json::from_str(r#"{"kind":"week_all"}"#).expect("week_all");
        let s: HistoryMetric = serde_json::from_str(r#"{"kind":"session"}"#).expect("session");
        let m: HistoryMetric =
            serde_json::from_str(r#"{"kind":"model","label":"Fable"}"#).expect("model");
        assert_eq!(w, HistoryMetric::WeekAll);
        assert_eq!(s, HistoryMetric::Session);
        assert_eq!(m, HistoryMetric::Model { label: "Fable".into() });
        assert!(serde_json::from_str::<HistoryMetric>(r#"{"kind":"nope"}"#).is_err());
    }

    #[test]
    fn history_buckets_are_anchored_at_since_and_take_the_maximum() {
        let (_tmp, store, acct) = store_with_account();
        // `since` deliberately NOT on an hour boundary: buckets start at since.
        let since = NOW + 12_345;
        let bucket = 15 * 60_000; // 15 minutes
        store.insert_snapshot(&acct, since + 1_000, &ok_outcome(1, 4), None, 1).expect("a");
        store.insert_snapshot(&acct, since + 2_000, &ok_outcome(1, 9), None, 1).expect("b");
        store.insert_snapshot(&acct, since + bucket + 1, &ok_outcome(1, 6), None, 1).expect("c");
        store.insert_snapshot(&acct, since - 1, &ok_outcome(1, 99), None, 1).expect("before since");

        let points = store
            .history(&acct, since, bucket, &HistoryMetric::WeekAll)
            .expect("history");
        assert_eq!(points, vec![
            HistoryPoint { t: since, pct: 9 },
            HistoryPoint { t: since + bucket, pct: 6 },
        ]);
    }

    #[test]
    fn history_leaves_empty_buckets_out_entirely() {
        let (_tmp, store, acct) = store_with_account();
        let since = NOW;
        let bucket = 3_600_000;
        store.insert_snapshot(&acct, since + 1000, &ok_outcome(1, 4), None, 1).expect("a");
        // Skip three hours entirely, as the process gate does overnight.
        store.insert_snapshot(&acct, since + 4 * bucket + 1000, &ok_outcome(1, 7), None, 1).expect("b");

        let points = store.history(&acct, since, bucket, &HistoryMetric::WeekAll).expect("history");
        assert_eq!(points.len(), 2, "no zero-filling of the gap");
        assert_eq!(points[0].t, since);
        assert_eq!(points[1].t, since + 4 * bucket);
    }

    #[test]
    fn history_ignores_non_ok_rows_for_every_metric() {
        let (_tmp, store, acct) = store_with_account();
        store.insert_snapshot(&acct, NOW + 1000, &PollOutcome::Timeout(30), None, 1).expect("a");
        store.insert_snapshot(&acct, NOW + 2000, &PollOutcome::NoUsageData, None, 1).expect("b");
        for metric in [
            HistoryMetric::WeekAll,
            HistoryMetric::Session,
            HistoryMetric::Model { label: "Fable".into() },
        ] {
            assert!(store.history(&acct, NOW, 60_000, &metric).expect("history").is_empty());
        }
    }

    #[test]
    fn history_session_metric_reads_session_pct() {
        let (_tmp, store, acct) = store_with_account();
        store.insert_snapshot(&acct, NOW + 1000, &ok_outcome(37, 4), None, 1).expect("a");
        store.insert_snapshot(&acct, NOW + 2000, &ok_outcome(52, 9), None, 1).expect("b");
        let points = store.history(&acct, NOW, 60_000, &HistoryMetric::Session).expect("history");
        assert_eq!(points, vec![HistoryPoint { t: NOW, pct: 52 }]);
    }

    #[test]
    fn history_model_metric_matches_the_label_exactly_and_ignores_other_models() {
        let (_tmp, store, acct) = store_with_account();
        store
            .insert_snapshot(&acct, NOW + 1000, &ok_with_models(1, 1, &[("Fable", 40), ("Opus", 90)]), None, 1)
            .expect("a");
        store
            .insert_snapshot(&acct, NOW + 2000, &ok_with_models(1, 1, &[("Fable", 45)]), None, 1)
            .expect("b");
        store
            .insert_snapshot(&acct, NOW + 3000, &ok_with_models(1, 1, &[("Opus", 95)]), None, 1)
            .expect("c: no Fable at all");

        let fable = store
            .history(&acct, NOW, 60_000, &HistoryMetric::Model { label: "Fable".into() })
            .expect("fable");
        assert_eq!(fable, vec![HistoryPoint { t: NOW, pct: 45 }]);

        let lower = store
            .history(&acct, NOW, 60_000, &HistoryMetric::Model { label: "fable".into() })
            .expect("case differs");
        assert!(lower.is_empty(), "label match is exact");
    }

    #[test]
    fn history_model_metric_tolerates_a_malformed_json_row_and_a_real_pct() {
        let (_tmp, store, acct) = store_with_account();
        store
            .insert_snapshot(&acct, NOW + 1000, &ok_with_models(1, 1, &[("Fable", 40)]), None, 1)
            .expect("good row");
        store
            .with_conn(|c| {
                c.execute(
                    "INSERT INTO snapshots(account_id, taken_at, outcome, week_models, duration_ms)
                     VALUES(?1, ?2, 'ok', '{not json', 1)",
                    params![acct, NOW + 2000],
                )?;
                c.execute(
                    "INSERT INTO snapshots(account_id, taken_at, outcome, week_models, duration_ms)
                     VALUES(?1, ?2, 'ok', '[{\"label\":\"Fable\",\"pct\":47.6,\"resets_at\":null}]', 1)",
                    params![acct, NOW + 3000],
                )?;
                // Valid JSON but not an array, and an array with a scalar
                // element plus a text pct: all must be ignored, never raise.
                c.execute(
                    "INSERT INTO snapshots(account_id, taken_at, outcome, week_models, duration_ms)
                     VALUES(?1, ?2, 'ok', '\"abc\"', 1)",
                    params![acct, NOW + 4000],
                )?;
                c.execute(
                    "INSERT INTO snapshots(account_id, taken_at, outcome, week_models, duration_ms)
                     VALUES(?1, ?2, 'ok', '[5, {\"label\":\"Fable\",\"pct\":\"99\"}]', 1)",
                    params![acct, NOW + 5000],
                )?;
                Ok(())
            })
            .expect("raw inserts");

        let points = store
            .history(&acct, NOW, 60_000, &HistoryMetric::Model { label: "Fable".into() })
            .expect("malformed and odd-shaped rows must not fail the query");
        assert_eq!(points, vec![HistoryPoint { t: NOW, pct: 48 }], "47.6 rounds to 48; text pct ignored");
        let labels = store.history_models(&acct, NOW).expect("labels must not fail either");
        assert_eq!(labels, vec!["Fable".to_string()]);
    }

    #[test]
    fn history_models_lists_distinct_sorted_labels_within_the_window() {
        let (_tmp, store, acct) = store_with_account();
        store
            .insert_snapshot(&acct, NOW + 1000, &ok_with_models(1, 1, &[("Opus", 1), ("Fable", 2)]), None, 1)
            .expect("a");
        store
            .insert_snapshot(&acct, NOW + 2000, &ok_with_models(1, 1, &[("Fable", 3)]), None, 1)
            .expect("b");
        store
            .insert_snapshot(&acct, NOW - 10, &ok_with_models(1, 1, &[("Ancient", 3)]), None, 1)
            .expect("before since");
        store
            .insert_snapshot(&acct, NOW + 3000, &PollOutcome::Timeout(30), None, 1)
            .expect("failure row has no models");
        let long = "x".repeat(65);
        store
            .insert_snapshot(&acct, NOW + 4000, &ok_with_models(1, 1, &[(long.as_str(), 1), (" \t ", 1)]), None, 1)
            .expect("unusable labels");

        let labels = store.history_models(&acct, NOW).expect("labels");
        assert_eq!(labels, vec!["Fable".to_string(), "Opus".to_string()]);
    }
```

- [ ] **Step 3: Write the failing command tests**

In `src-tauri/src/commands.rs` `mod tests`, delete `get_history_returns_hourly_points` and `history_days_is_clamped_to_one_through_thirty`, add `use crate::store::{HistoryMetric, MAX_BUCKETS, MAX_LABEL_LEN, MAX_RANGE_MS, MIN_BUCKET_MS, RANGE_SLACK_MS};` to the module's `use` lines, and add:

```rust
    fn ok_week(pct: u8) -> PollOutcome {
        PollOutcome::Ok(crate::usage::Parsed {
            session: crate::usage::Window { pct: 1, resets_at: None },
            week_all: crate::usage::Window { pct, resets_at: None },
            week_models: vec![("Fable".to_string(), crate::usage::Window { pct: 7, resets_at: None })],
        })
    }

    #[test]
    fn get_history_buckets_the_requested_metric_from_since() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &d, 1).expect("add");
        let since = 1_000_000_000i64;
        for (t, pct) in [(since + 1000, 4u8), (since + 2000, 9), (since + 3_600_000, 6)] {
            core.store.insert_snapshot(&a.id, t, &ok_week(pct), None, 1).expect("snapshot");
        }
        let now = since + 8 * 3_600_000;
        let points = core_get_history(&core, &a.id, now, since, 3_600_000, &HistoryMetric::WeekAll).expect("history");
        assert_eq!(points.len(), 2);
        assert_eq!((points[0].t, points[0].pct), (since, 9));
        assert_eq!((points[1].t, points[1].pct), (since + 3_600_000, 6));

        let fable = core_get_history(&core, &a.id, now, since, 3_600_000, &HistoryMetric::Model { label: "Fable".into() }).expect("model");
        assert_eq!(fable.iter().map(|p| p.pct).collect::<Vec<u8>>(), vec![7, 7]);
    }

    #[test]
    fn get_history_rejects_each_limit_at_the_boundary_and_accepts_one_step_inside() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &d, 1).expect("add");
        let now = 10_000_000_000i64;
        let week = HistoryMetric::WeekAll;
        let code = |r: AppResult<Vec<HistoryPoint>>| r.map(|_| ()).map_err(|e| e.code());

        // bucket_ms
        assert_eq!(code(core_get_history(&core, &a.id, now, now - 3_600_000, MIN_BUCKET_MS - 1, &week)), Err("out_of_range"));
        assert_eq!(code(core_get_history(&core, &a.id, now, now - 3_600_000, MIN_BUCKET_MS, &week)), Ok(()));
        // since in the future
        assert_eq!(code(core_get_history(&core, &a.id, now, now + 1, MIN_BUCKET_MS, &week)), Err("out_of_range"));
        assert_eq!(code(core_get_history(&core, &a.id, now, now, MIN_BUCKET_MS, &week)), Ok(()));
        // range (with slack)
        let limit = MAX_RANGE_MS + RANGE_SLACK_MS;
        assert_eq!(code(core_get_history(&core, &a.id, now, now - limit - 1, 86_400_000, &week)), Err("out_of_range"));
        assert_eq!(code(core_get_history(&core, &a.id, now, now - limit, 86_400_000, &week)), Ok(()));
        // bucket count: 1000 * 60s = 60_000_000 ms of range at the minimum bucket
        let full = MAX_BUCKETS * MIN_BUCKET_MS;
        assert_eq!(code(core_get_history(&core, &a.id, now, now - full - 1, MIN_BUCKET_MS, &week)), Err("out_of_range"));
        assert_eq!(code(core_get_history(&core, &a.id, now, now - full, MIN_BUCKET_MS, &week)), Ok(()));
        // labels
        let blank = HistoryMetric::Model { label: " \t ".into() };
        let long = HistoryMetric::Model { label: "é".repeat(MAX_LABEL_LEN + 1) };
        let max = HistoryMetric::Model { label: "é".repeat(MAX_LABEL_LEN) };
        assert_eq!(code(core_get_history(&core, &a.id, now, now - 3_600_000, MIN_BUCKET_MS, &blank)), Err("out_of_range"));
        assert_eq!(code(core_get_history(&core, &a.id, now, now - 3_600_000, MIN_BUCKET_MS, &long)), Err("out_of_range"));
        assert_eq!(code(core_get_history(&core, &a.id, now, now - 3_600_000, MIN_BUCKET_MS, &max)), Ok(()), "64 chars, not bytes");
    }

    #[test]
    fn get_history_models_lists_labels_within_retention() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &d, 1).expect("add");
        let now = 10_000_000_000i64;
        core.store.insert_snapshot(&a.id, now - 1000, &ok_week(1), None, 1).expect("recent");
        core.store
            .insert_snapshot(&a.id, now - crate::store::RETENTION_MS - 1, &ok_week(1), None, 1)
            .expect("too old (would already be pruned in production)");
        assert_eq!(core_get_history_models(&core, &a.id, now).expect("labels"), vec!["Fable".to_string()]);
    }
```

- [ ] **Step 4: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: compile errors (`HistoryMetric`, the constants and `history_models` undefined; `history` / `core_get_history` arity).

- [ ] **Step 5: Implement the store**

In `src-tauri/src/store/snapshots.rs`, replace the imports and constants at the top:

```rust
use rusqlite::{params, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::warn;

use crate::error::{AppError, AppResult};
use crate::store::Store;
use crate::usage::{ModelWindow, OutcomeKind, PollOutcome, SnapshotDto, Window};

/// D10: 30 days, in milliseconds.
pub const RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;

// History query limits (spec §3.1). The first three are mirrored in
// src/lib/historyLimits.json for the UI; a test below pins them together.
pub const MIN_BUCKET_MS: i64 = 60_000;
pub const MAX_BUCKETS: i64 = 1_000;
/// 30 days; equals RETENTION_MS (asserted in tests). Digit literal on purpose.
pub const MAX_RANGE_MS: i64 = 2_592_000_000;
/// Grace for a client that computed `since` a little before the command ran.
pub const RANGE_SLACK_MS: i64 = 300_000;
/// Model labels longer than this are neither offered nor accepted.
pub const MAX_LABEL_LEN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HistoryPoint {
    pub t: i64,
    pub pct: u8,
}

/// Which stored value a history query buckets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryMetric {
    WeekAll,
    Session,
    Model { label: String },
}
```

Delete `const HOUR_MS`. Replace `Store::history` with:

```rust
    /// Buckets of `MAX(metric)` over `ok` rows, anchored at `since`
    /// (`bucket = since + floor((taken_at - since) / bucket_ms) * bucket_ms`),
    /// only for buckets that have at least one row. No zero-filling: the
    /// process gate guarantees overnight gaps and those must render as
    /// breaks. Limits are enforced by the caller (`commands::core_get_history`).
    pub fn history(
        &self,
        account_id: &str,
        since: i64,
        bucket_ms: i64,
        metric: &HistoryMetric,
    ) -> AppResult<Vec<HistoryPoint>> {
        const SCALAR: &str = "SELECT ?2 + ((taken_at - ?2) / ?3) * ?3 AS bucket, MAX({col}) AS pct
             FROM snapshots
             WHERE account_id = ?1 AND taken_at >= ?2
               AND outcome = 'ok' AND {col} IS NOT NULL
             GROUP BY bucket
             ORDER BY bucket ASC";
        // `json_each` raises on malformed input BEFORE any WHERE term can
        // filter the row, so the guard lives in its argument. Element
        // fields are read from the root document via `m.fullkey` so a
        // scalar element ("abc" instead of {…}) yields NULL, never an error.
        const MODEL: &str = "SELECT ?2 + ((s.taken_at - ?2) / ?3) * ?3 AS bucket,
                    MAX(json_extract(s.week_models, m.fullkey || '.pct')) AS pct
             FROM snapshots AS s,
                  json_each(CASE WHEN json_valid(s.week_models) AND json_type(s.week_models) = 'array'
                                 THEN s.week_models ELSE '[]' END) AS m
             WHERE s.account_id = ?1 AND s.taken_at >= ?2
               AND s.outcome = 'ok' AND s.week_models IS NOT NULL
               AND json_extract(s.week_models, m.fullkey || '.label') = ?4
               AND json_type(s.week_models, m.fullkey || '.pct') IN ('integer', 'real')
             GROUP BY bucket
             ORDER BY bucket ASC";

        let (sql, label): (String, Option<&str>) = match metric {
            HistoryMetric::WeekAll => (SCALAR.replace("{col}", "week_all_pct"), None),
            HistoryMetric::Session => (SCALAR.replace("{col}", "session_pct"), None),
            HistoryMetric::Model { label } => (MODEL.to_string(), Some(label.as_str())),
        };

        self.with_conn(|c| {
            let mut stmt = c.prepare(&sql)?;
            let map = |r: &Row<'_>| -> Result<HistoryPoint, rusqlite::Error> {
                // JSON numbers may come back as REAL; integers widen losslessly.
                let pct: f64 = r.get("pct")?;
                Ok(HistoryPoint {
                    t: r.get::<_, i64>("bucket")?,
                    pct: pct.round().clamp(0.0, 100.0) as u8,
                })
            };
            let rows = match label {
                Some(l) => stmt
                    .query_map(params![account_id, since, bucket_ms, l], map)?
                    .collect::<Result<Vec<HistoryPoint>, rusqlite::Error>>()?,
                None => stmt
                    .query_map(params![account_id, since, bucket_ms], map)?
                    .collect::<Result<Vec<HistoryPoint>, rusqlite::Error>>()?,
            };
            Ok(rows)
        })
    }

    /// Distinct model labels seen in `ok` rows since `since`, sorted. Only
    /// labels the metric validation would accept (non-blank after trimming
    /// space/tab/LF/CR, at most `MAX_LABEL_LEN` characters) are returned, so
    /// the picker can never offer something `history` would reject.
    pub fn history_models(&self, account_id: &str, since: i64) -> AppResult<Vec<String>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT DISTINCT json_extract(s.week_models, m.fullkey || '.label') AS label
                 FROM snapshots AS s,
                      json_each(CASE WHEN json_valid(s.week_models) AND json_type(s.week_models) = 'array'
                                     THEN s.week_models ELSE '[]' END) AS m
                 WHERE s.account_id = ?1 AND s.taken_at >= ?2
                   AND s.outcome = 'ok' AND s.week_models IS NOT NULL
                   AND typeof(label) = 'text'
                   AND length(trim(label, char(32, 9, 10, 13))) >= 1
                   AND length(label) <= ?3
                 ORDER BY label ASC",
            )?;
            let rows = stmt
                .query_map(params![account_id, since, MAX_LABEL_LEN as i64], |r| r.get::<_, String>("label"))?
                .collect::<Result<Vec<String>, rusqlite::Error>>()?;
            Ok(rows)
        })
    }
```

`trim(label, char(32, 9, 10, 13))` strips space, tab, LF and CR; SQLite's bare `trim()` strips spaces only.

In `src-tauri/src/store/mod.rs`, replace `pub use snapshots::HistoryPoint;` with:

```rust
pub use snapshots::{
    HistoryMetric, HistoryPoint, MAX_BUCKETS, MAX_LABEL_LEN, MAX_RANGE_MS, MIN_BUCKET_MS,
    RANGE_SLACK_MS, RETENTION_MS,
};
```

- [ ] **Step 6: Implement the commands**

In `src-tauri/src/commands.rs`:

- Change `use tracing::{info, warn};` to `use tracing::{debug, info, warn};` and `use crate::store::{HistoryPoint, Store};` to `use crate::store::{HistoryMetric, HistoryPoint, Store, MAX_BUCKETS, MAX_LABEL_LEN, MAX_RANGE_MS, MIN_BUCKET_MS, RANGE_SLACK_MS, RETENTION_MS};`.
- Delete `MAX_HISTORY_DAYS`, `DEFAULT_HISTORY_DAYS`, `DAY_MS` and the old `core_get_history`; replace with:

```rust
/// Spec §3.1: every limit is checked here, never in the store. Errors name
/// the argument and the bound so a mis-built request is diagnosable from
/// the toast alone.
fn validate_history_request(now: i64, since: i64, bucket_ms: i64, metric: &HistoryMetric) -> AppResult<()> {
    if bucket_ms < MIN_BUCKET_MS {
        return Err(AppError::OutOfRange(format!("bucket_ms must be >= {MIN_BUCKET_MS}, got {bucket_ms}")));
    }
    if since > now {
        return Err(AppError::OutOfRange(format!("since must not be in the future (since={since}, now={now})")));
    }
    let range = now - since;
    if range > MAX_RANGE_MS + RANGE_SLACK_MS {
        return Err(AppError::OutOfRange(format!("range must be <= {MAX_RANGE_MS} ms (+{RANGE_SLACK_MS} slack), got {range}")));
    }
    let buckets = (range + bucket_ms - 1) / bucket_ms;
    if buckets > MAX_BUCKETS {
        return Err(AppError::OutOfRange(format!("request spans {buckets} buckets; max is {MAX_BUCKETS}")));
    }
    if let HistoryMetric::Model { label } = metric {
        let trimmed = label.trim();
        if trimmed.is_empty() {
            return Err(AppError::OutOfRange("model label must not be blank".into()));
        }
        if trimmed.chars().count() > MAX_LABEL_LEN {
            return Err(AppError::OutOfRange(format!("model label must be <= {MAX_LABEL_LEN} characters")));
        }
    }
    Ok(())
}

pub fn core_get_history(
    core: &Core,
    account_id: &str,
    now: i64,
    since: i64,
    bucket_ms: i64,
    metric: &HistoryMetric,
) -> AppResult<Vec<HistoryPoint>> {
    if let Err(e) = validate_history_request(now, since, bucket_ms, metric) {
        warn!(account_id, since, bucket_ms, ?metric, error = %e, "history request rejected");
        return Err(e);
    }
    let points = core.store.history(account_id, since, bucket_ms, metric)?;
    debug!(account_id, since, bucket_ms, ?metric, points = points.len(), "history");
    Ok(points)
}

pub fn core_get_history_models(core: &Core, account_id: &str, now: i64) -> AppResult<Vec<String>> {
    let labels = core.store.history_models(account_id, now - RETENTION_MS)?;
    debug!(account_id, labels = labels.len(), "history models");
    Ok(labels)
}
```

- Replace the `get_history` command and add `get_history_models`:

```rust
#[tauri::command]
pub async fn get_history(
    core: State<'_, SharedCore>,
    account_id: String,
    since: i64,
    bucket_ms: i64,
    metric: HistoryMetric,
) -> AppResult<Vec<HistoryPoint>> {
    let core = Arc::clone(&core);
    blocking(move || core_get_history(&core, &account_id, now_ms(), since, bucket_ms, &metric)).await
}

#[tauri::command]
pub async fn get_history_models(
    core: State<'_, SharedCore>,
    account_id: String,
) -> AppResult<Vec<String>> {
    let core = Arc::clone(&core);
    blocking(move || core_get_history_models(&core, &account_id, now_ms())).await
}
```

- In `src-tauri/src/lib.rs` add `commands::get_history_models,` right after `commands::get_history,` in `generate_handler!`.

- [ ] **Step 7: Run all four gates**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`, `npm test`, `npm run build`
Expected: all green. `grep -rn "HOUR_MS\|MAX_HISTORY_DAYS\|DEFAULT_HISTORY_DAYS" src-tauri/src` returns nothing. (The frontend still sends the old `days` argument; Task 5 changes it. Until then a real build's sparkline request would be rejected, which is why Tasks 1–3 and 4–6 merge together before any real-window check.)

- [ ] **Step 8: Commit**

```bash
git add src/lib/historyLimits.json src-tauri/src
git commit -m "feat(history): since-anchored bucketing by metric with server-side limits"
```

---

### Task 3: Remove the default flag from the frontend and drop the hint line

**Files:**
- Modify: `src/lib/types.ts`, `src/components/AccountRow.tsx`, `src/lib/mockBackend.ts`, `src/App.tsx`, `src/styles.css`, and every `src/lib/*.test.ts` that builds an `Account` literal (`present.test.ts` for certain; run `grep -rln is_default src` to find the rest)

**Interfaces:**
- Produces: `Account` (TS) has no `is_default`. `App.tsx` renders no `<p className="hint">` under the table.

- [ ] **Step 1: Remove the field and its uses**

- `src/lib/types.ts`: delete `is_default: boolean;` from `Account`.
- `src/components/AccountRow.tsx`: delete the line `{row.account.is_default && <span className="tag">default</span>}`.
- `src/lib/mockBackend.ts`: `makeAccount(id: string, sortOrder: number, now: number)` without the `isDefault` parameter and field; the three calls become `makeAccount("claude3", 0, now)`, `makeAccount("claude", 1, now)`, `makeAccount("claude2", 2, now)`; delete `is_default: false,` in `add_account`.
- Every test literal: delete `is_default: false,` (e.g. `src/lib/present.test.ts` `row()` helper).
- `src/styles.css`: delete the `.tag { … }` rule and the `--tag-fg: #9fb6d3; --tag-border: #2c3a49; --tag-bg: #161d24;` line in `:root`.
- `src/App.tsx`: delete `<p className="hint">drag a row handle to set failover priority · drag a column header to reorder columns</p>`. Keep the `.hint` CSS class (Settings hints use it).

- [ ] **Step 2: Run all four gates**

Run: `npm test`, `npm run build`, `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
Expected: green; `grep -rn "is_default\|isDefault" src` returns nothing.

- [ ] **Step 3: Commit**

```bash
git add src
git commit -m "refactor(ui): drop the default tag and the table hint line"
```

---

### Task 4: `history.ts` — presets, alignment, bucketing, labels (pure)

**Files:**
- Create: `src/lib/history.ts`
- Test: `src/lib/history.test.ts`

**Interfaces:**
- Consumes: `src/lib/historyLimits.json`, `HistoryPoint` from `types.ts`.
- Produces (all exported from `src/lib/history.ts`):
  - `MIN_BUCKET_MS`, `MAX_BUCKETS`, `MAX_RANGE_MS: number`
  - `type PresetKey = "1h"|"6h"|"12h"|"24h"|"7d"|"30d"`, `type UnitKey = "1m"|"5m"|"15m"|"1h"|"1d"`
  - `PRESETS: Record<PresetKey, { label: string; rangeMs: number; autoUnit: UnitKey }>`, `PRESET_KEYS`, `UNITS: Record<UnitKey, { label: string; ms: number }>`, `UNIT_KEYS`
  - `type Metric = { kind: "week_all" } | { kind: "session" } | { kind: "model"; label: string }`, `WEEK_ALL: Metric`, `metricLabel(m)`, `metricKey(m)`, `metricFromKey(key: string): Metric`
  - `unitAllowed(preset, unit)`, `effectiveUnit(preset, override)`, `alignedSince(now, preset, unit)`, `bucketCount(since, now, unit)`, `bucketSeries(points, since, unit, count)`, `axisLabelsFor(since, unit, count, labels, locale?)`, `slotLabel(since, unit, index, locale?)`, `missingLabel(missing, unit)`, `showMissing(unit)`, `nearestKnownSlot(vals, fraction)`
- `series.ts` is untouched in this task (Task 6 trims it once nothing imports the old helpers).

- [ ] **Step 1: Write the failing tests**

`src/lib/history.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import limits from "./historyLimits.json";
import {
  MAX_BUCKETS, MAX_RANGE_MS, MIN_BUCKET_MS, PRESETS, PRESET_KEYS, UNITS, UNIT_KEYS, WEEK_ALL,
  alignedSince, axisLabelsFor, bucketCount, bucketSeries, effectiveUnit, metricFromKey, metricKey,
  metricLabel, missingLabel, nearestKnownSlot, showMissing, slotLabel, unitAllowed,
} from "./history";

const MIN = 60_000;
const HOUR = 3_600_000;
const DAY = 86_400_000;
// 2026-09-16 15:07:30.250 local
const NOW = new Date(2026, 8, 16, 15, 7, 30, 250).getTime();

describe("limits", () => {
  it("come from the shared JSON the Rust side is tested against", () => {
    expect(MIN_BUCKET_MS).toBe(limits.minBucketMs);
    expect(MAX_BUCKETS).toBe(limits.maxBuckets);
    expect(MAX_RANGE_MS).toBe(limits.maxRangeMs);
    expect(MIN_BUCKET_MS).toBe(60_000);
    expect(MAX_BUCKETS).toBe(1000);
    expect(MAX_RANGE_MS).toBe(30 * DAY);
  });
});

describe("presets and units", () => {
  it("match spec §3.2 exactly", () => {
    expect(PRESET_KEYS).toEqual(["1h", "6h", "12h", "24h", "7d", "30d"]);
    expect(UNIT_KEYS).toEqual(["1m", "5m", "15m", "1h", "1d"]);
    expect(Object.fromEntries(PRESET_KEYS.map((p) => [p, PRESETS[p].autoUnit]))).toEqual({
      "1h": "1m", "6h": "5m", "12h": "15m", "24h": "15m", "7d": "1h", "30d": "1d",
    });
    expect(Object.fromEntries(UNIT_KEYS.map((u) => [u, UNITS[u].ms]))).toEqual({
      "1m": MIN, "5m": 5 * MIN, "15m": 15 * MIN, "1h": HOUR, "1d": DAY,
    });
    const allowed = Object.fromEntries(PRESET_KEYS.map((p) => [p, UNIT_KEYS.filter((u) => unitAllowed(p, u))]));
    expect(allowed).toEqual({
      "1h": ["1m", "5m", "15m"],
      "6h": ["1m", "5m", "15m", "1h"],
      "12h": ["1m", "5m", "15m", "1h"],
      "24h": ["5m", "15m", "1h"],
      "7d": ["15m", "1h", "1d"],
      "30d": ["1h", "1d"],
    });
  });
  it("every auto unit is allowed and every allowed pair respects both server limits", () => {
    for (const p of PRESET_KEYS) {
      expect(unitAllowed(p, PRESETS[p].autoUnit)).toBe(true);
      for (const u of UNIT_KEYS) {
        if (!unitAllowed(p, u)) continue;
        expect(UNITS[u].ms).toBeGreaterThanOrEqual(MIN_BUCKET_MS);
        expect(Math.ceil(PRESETS[p].rangeMs / UNITS[u].ms)).toBeLessThanOrEqual(MAX_BUCKETS);
        expect(PRESETS[p].rangeMs).toBeLessThanOrEqual(MAX_RANGE_MS);
      }
    }
  });
  it("effectiveUnit honours an allowed override and falls back to auto otherwise", () => {
    expect(effectiveUnit("7d", null)).toBe("1h");
    expect(effectiveUnit("7d", "1d")).toBe("1d");
    expect(effectiveUnit("7d", "1m")).toBe("1h");
  });
});

describe("metric helpers", () => {
  it("labels, keys and parses metrics", () => {
    expect(metricLabel(WEEK_ALL)).toBe("weekly limit");
    expect(metricLabel({ kind: "session" })).toBe("session");
    expect(metricLabel({ kind: "model", label: "Fable" })).toBe("Fable");
    expect(metricKey({ kind: "model", label: "Fable" })).toBe("model:Fable");
    expect(metricKey(WEEK_ALL)).toBe("week_all");
    expect(metricFromKey("session")).toEqual({ kind: "session" });
    expect(metricFromKey("model:Fable")).toEqual({ kind: "model", label: "Fable" });
    expect(metricFromKey("model:")).toEqual(WEEK_ALL);
    expect(metricFromKey("bogus")).toEqual(WEEK_ALL);
  });
});

describe("alignedSince", () => {
  it("rounds up to the next unit boundary in local time and never exceeds the range", () => {
    const since1m = alignedSince(NOW, "1h", "1m");
    expect(new Date(since1m).getSeconds()).toBe(0);
    expect(new Date(since1m).getMilliseconds()).toBe(0);
    expect(NOW - since1m).toBeLessThanOrEqual(HOUR);
    expect(NOW - since1m).toBeGreaterThan(HOUR - MIN);

    const since15 = alignedSince(NOW, "12h", "15m");
    expect(new Date(since15).getMinutes() % 15).toBe(0);
    expect(NOW - since15).toBeLessThanOrEqual(12 * HOUR);

    const sinceH = alignedSince(NOW, "7d", "1h");
    expect(new Date(sinceH).getMinutes()).toBe(0);
    expect(NOW - sinceH).toBeLessThanOrEqual(7 * DAY);

    const sinceD = alignedSince(NOW, "30d", "1d");
    const d = new Date(sinceD);
    expect([d.getHours(), d.getMinutes()]).toEqual([0, 0]);
    expect(NOW - sinceD).toBeLessThanOrEqual(30 * DAY);
  });
  it("leaves a value already on the boundary where it is", () => {
    const onHour = new Date(2026, 8, 16, 15, 0, 0, 0).getTime();
    expect(alignedSince(onHour + HOUR, "1h", "1m")).toBe(onHour);
    const midnight = new Date(2026, 8, 16, 0, 0, 0, 0).getTime();
    expect(alignedSince(midnight + 7 * DAY, "7d", "1d")).toBe(midnight);
  });
  it("lands on the local day start for day units across a whole year (covers any DST change)", () => {
    for (let i = 0; i < 40; i++) {
      const now = new Date(2026, 0, 3 + i * 9, 13, 21, 0, 0).getTime();
      const s = alignedSince(now, "30d", "1d");
      // Compare against the day start recomputed from the result itself, so a
      // zone whose spring-forward happens at 00:00 (no midnight that day) passes.
      const dayStart = new Date(s);
      dayStart.setHours(0, 0, 0, 0);
      expect(s).toBe(dayStart.getTime());
      expect(now - s).toBeLessThanOrEqual(30 * DAY);
    }
  });
  it("bucketCount never exceeds MAX_BUCKETS and is at least 2 for any allowed pair", () => {
    for (const p of PRESET_KEYS) for (const u of UNIT_KEYS) {
      if (!unitAllowed(p, u)) continue;
      const since = alignedSince(NOW, p, u);
      expect(bucketCount(since, NOW, u)).toBeLessThanOrEqual(MAX_BUCKETS);
      expect(bucketCount(since, NOW, u)).toBeGreaterThanOrEqual(2);
    }
  });
});

describe("bucketSeries", () => {
  const since = 1_000_000;
  it("puts each point in its slot, keeps the max, leaves gaps null, ignores out-of-range", () => {
    const vals = bucketSeries(
      [
        { t: since, pct: 10 },
        { t: since + 2 * MIN, pct: 30 },
        { t: since + 2 * MIN + 1, pct: 25 },
        { t: since - 1, pct: 99 },
        { t: since + 4 * MIN, pct: 50 },
      ],
      since, "1m", 4,
    );
    expect(vals).toEqual([10, null, 30, null]);
  });
  it("is all null for no points", () => {
    expect(bucketSeries([], since, "1h", 3)).toEqual([null, null, null]);
  });
});

describe("labels", () => {
  const midnight = new Date(2026, 8, 16, 0, 0, 0, 0).getTime();
  const hour = new Date(2026, 8, 16, 14, 0, 0, 0).getTime();
  it("uses times for sub-day ranges and dates otherwise, first and last always present", () => {
    expect(axisLabelsFor(hour, "1m", 60, 4, "en-US")).toEqual(["14:00", "14:20", "14:39", "14:59"]);
    expect(axisLabelsFor(midnight - 6 * DAY, "1d", 7, 4, "en-US")).toEqual(["Sep 10", "Sep 12", "Sep 14", "Sep 16"]);
    expect(axisLabelsFor(midnight - 6 * DAY, "1h", 168, 2, "en-US")).toEqual(["Sep 10", "Sep 16"]);
    expect(axisLabelsFor(hour, "15m", 96, 1, "en-US")).toHaveLength(2);
    // Fewer slots than requested labels: one label per slot, no repeats.
    expect(axisLabelsFor(hour, "15m", 3, 4, "en-US")).toEqual(["14:00", "14:15", "14:30"]);
  });
  it("slot labels carry date and time for sub-day units and the date only for days", () => {
    expect(slotLabel(hour, "15m", 5, "en-US")).toBe("Sep 16 15:15");
    expect(slotLabel(midnight - 2 * DAY, "1d", 2, "en-US")).toBe("Sep 16");
  });
  it("missing label is unit aware and the stat is hidden at 1m", () => {
    expect(missingLabel(3, "1m")).toBe("3 m");
    expect(missingLabel(3, "5m")).toBe("15 m");
    expect(missingLabel(2, "15m")).toBe("30 m");
    expect(missingLabel(4, "1h")).toBe("4 h");
    expect(missingLabel(2, "1d")).toBe("2 d");
    expect(showMissing("1m")).toBe(false);
    expect(showMissing("5m")).toBe(true);
    expect(showMissing("1d")).toBe(true);
  });
});

describe("nearestKnownSlot", () => {
  it("maps the fraction via round(f * (n-1)) then walks outward, lower index first on ties", () => {
    expect(nearestKnownSlot([1, 2, 3, 4, 5], 0.5)).toBe(2);
    expect(nearestKnownSlot([1, 2, null, 4, 5], 0.5)).toBe(1);
    expect(nearestKnownSlot([1, null, null, null, 5], 0.5)).toBe(0);
    expect(nearestKnownSlot([null, null, 3], 0)).toBe(2);
    expect(nearestKnownSlot([7], 0.9)).toBe(0);
    expect(nearestKnownSlot([null, null], 0.3)).toBeNull();
    expect(nearestKnownSlot([], 0.3)).toBeNull();
    expect(nearestKnownSlot([1, 2, 3], 1.7)).toBe(2);
    expect(nearestKnownSlot([1, 2, 3], -3)).toBe(0);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npm test -- src/lib/history.test.ts`
Expected: FAIL — cannot resolve `./history`.

- [ ] **Step 3: Implement**

`src/lib/history.ts`:

```ts
import limits from "./historyLimits.json";
import type { HistoryPoint } from "./types";

/** Shared with src-tauri/src/store/snapshots.rs via historyLimits.json. */
export const MIN_BUCKET_MS: number = limits.minBucketMs;
export const MAX_BUCKETS: number = limits.maxBuckets;
export const MAX_RANGE_MS: number = limits.maxRangeMs;

export type PresetKey = "1h" | "6h" | "12h" | "24h" | "7d" | "30d";
export type UnitKey = "1m" | "5m" | "15m" | "1h" | "1d";

export interface Preset { label: string; rangeMs: number; autoUnit: UnitKey }
export interface Unit { label: string; ms: number }

const MINUTE = 60_000;
const HOUR = 3_600_000;
const DAY = 86_400_000;

export const UNITS: Record<UnitKey, Unit> = {
  "1m": { label: "1m", ms: MINUTE },
  "5m": { label: "5m", ms: 5 * MINUTE },
  "15m": { label: "15m", ms: 15 * MINUTE },
  "1h": { label: "1h", ms: HOUR },
  "1d": { label: "1d", ms: DAY },
};
export const UNIT_KEYS: readonly UnitKey[] = ["1m", "5m", "15m", "1h", "1d"];

export const PRESETS: Record<PresetKey, Preset> = {
  "1h": { label: "1 h", rangeMs: HOUR, autoUnit: "1m" },
  "6h": { label: "6 h", rangeMs: 6 * HOUR, autoUnit: "5m" },
  "12h": { label: "12 h", rangeMs: 12 * HOUR, autoUnit: "15m" },
  "24h": { label: "24 h", rangeMs: DAY, autoUnit: "15m" },
  "7d": { label: "7 days", rangeMs: 7 * DAY, autoUnit: "1h" },
  "30d": { label: "30 days", rangeMs: 30 * DAY, autoUnit: "1d" },
};
export const PRESET_KEYS: readonly PresetKey[] = ["1h", "6h", "12h", "24h", "7d", "30d"];

/** Wire shape of the Rust `HistoryMetric` (serde tag = "kind"). */
export type Metric =
  | { kind: "week_all" }
  | { kind: "session" }
  | { kind: "model"; label: string };

export const WEEK_ALL: Metric = { kind: "week_all" };

export function metricLabel(m: Metric): string {
  switch (m.kind) {
    case "week_all": return "weekly limit";
    case "session": return "session";
    case "model": return m.label;
  }
}

/** Stable string for React keys and <select> values. */
export function metricKey(m: Metric): string {
  return m.kind === "model" ? `model:${m.label}` : m.kind;
}

/** Inverse of metricKey; anything unrecognised falls back to week-all. */
export function metricFromKey(key: string): Metric {
  if (key === "session") return { kind: "session" };
  if (key.startsWith("model:") && key.length > "model:".length) {
    return { kind: "model", label: key.slice("model:".length) };
  }
  return WEEK_ALL;
}

/** At least two buckets and at most MAX_BUCKETS (spec §3.2). */
export function unitAllowed(preset: PresetKey, unit: UnitKey): boolean {
  const range = PRESETS[preset].rangeMs;
  const ms = UNITS[unit].ms;
  return range / ms >= 2 && Math.ceil(range / ms) <= MAX_BUCKETS;
}

export function effectiveUnit(preset: PresetKey, override: UnitKey | null): UnitKey {
  return override !== null && unitAllowed(preset, override) ? override : PRESETS[preset].autoUnit;
}

/**
 * `now - range`, rounded UP to the next unit boundary in local time (a value
 * already on a boundary stays put). Rounding up keeps `now - since <= range`
 * so the server's 30-day limit can never be crossed; the partial first unit
 * is dropped, not stretched.
 */
export function alignedSince(now: number, preset: PresetKey, unit: UnitKey): number {
  const raw = now - PRESETS[preset].rangeMs;
  if (unit === "1d") {
    const d = new Date(raw);
    d.setHours(0, 0, 0, 0);
    if (d.getTime() < raw) d.setDate(d.getDate() + 1);
    return d.getTime();
  }
  const hourStart = new Date(raw);
  hourStart.setMinutes(0, 0, 0);
  const ms = UNITS[unit].ms;
  const offset = raw - hourStart.getTime();
  return hourStart.getTime() + Math.ceil(offset / ms) * ms;
}

/** Slots from `since` up to and including the current, partial unit. */
export function bucketCount(since: number, now: number, unit: UnitKey): number {
  return Math.max(1, Math.ceil((now - since) / UNITS[unit].ms));
}

/** slot i = max pct of points with t in [since + i*ms, since + (i+1)*ms); null when none. */
export function bucketSeries(
  points: readonly HistoryPoint[],
  since: number,
  unit: UnitKey,
  count: number,
): Array<number | null> {
  const ms = UNITS[unit].ms;
  const out: Array<number | null> = Array.from({ length: count }, () => null);
  for (const p of points) {
    const idx = Math.floor((p.t - since) / ms);
    if (idx < 0 || idx >= count) continue;
    const cur = out[idx];
    out[idx] = cur === null ? p.pct : Math.max(cur, p.pct);
  }
  return out;
}

/** Start instant of slot `index`; day slots step by calendar day (DST-safe). */
function slotStart(since: number, unit: UnitKey, index: number): number {
  if (unit === "1d") {
    const d = new Date(since);
    d.setDate(d.getDate() + index);
    return d.getTime();
  }
  return since + index * UNITS[unit].ms;
}

function dateText(t: number, locale?: string): string {
  return new Date(t).toLocaleDateString(locale, { month: "short", day: "numeric" });
}

function timeText(t: number, locale?: string): string {
  return new Date(t).toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
}

/** `labels` evenly spaced slot labels, first and last always included. */
export function axisLabelsFor(
  since: number,
  unit: UnitKey,
  count: number,
  labels: number,
  locale?: string,
): string[] {
  // Never more labels than slots, or a short series repeats a label.
  const c = Math.max(2, Math.min(labels, count));
  const useDate = unit === "1d" || count * UNITS[unit].ms >= 2 * DAY;
  return Array.from({ length: c }, (_, i) => {
    const index = Math.round((i * (count - 1)) / (c - 1));
    const t = slotStart(since, unit, index);
    return useDate ? dateText(t, locale) : timeText(t, locale);
  });
}

export function slotLabel(since: number, unit: UnitKey, index: number, locale?: string): string {
  const t = slotStart(since, unit, index);
  return unit === "1d" ? dateText(t, locale) : `${dateText(t, locale)} ${timeText(t, locale)}`;
}

/** "12 m" | "3 h" | "2 d" for the stats strip. */
export function missingLabel(missing: number, unit: UnitKey): string {
  switch (unit) {
    case "1m": return `${missing} m`;
    case "5m": return `${missing * 5} m`;
    case "15m": return `${missing * 15} m`;
    case "1h": return `${missing} h`;
    case "1d": return `${missing} d`;
  }
}

/** Poll density is one row per 1.7–3.4 min, so at 1m gaps are expected, not missing. */
export function showMissing(unit: UnitKey): boolean {
  return unit !== "1m";
}

/**
 * polylineRuns places slot i at x = i/(n-1), so the candidate is
 * round(fraction * (n-1)); from there the nearest non-null slot wins, the
 * lower index on ties. Null when every slot is null.
 */
export function nearestKnownSlot(vals: readonly (number | null)[], fraction: number): number | null {
  const n = vals.length;
  if (n === 0) return null;
  const f = Math.min(1, Math.max(0, fraction));
  const start = n === 1 ? 0 : Math.round(f * (n - 1));
  if (vals[start] !== null) return start;
  for (let d = 1; d < n; d++) {
    const lo = start - d;
    const hi = start + d;
    if (lo >= 0 && vals[lo] !== null) return lo;
    if (hi < n && vals[hi] !== null) return hi;
  }
  return null;
}
```

- [ ] **Step 4: Run all four gates**

Run: `npm test`, `npm run build`, `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
Expected: green (this task only adds a module).

- [ ] **Step 5: Commit**

```bash
git add src/lib/history.ts src/lib/history.test.ts
git commit -m "feat(ui): history presets, alignment and bucketing helpers"
```

---

### Task 5: Backend seam, mock bucketing, dashboard hook and the `cycle` counter

**Files:**
- Modify: `src/lib/backend.ts`, `src/lib/mockBackend.ts`, `src/hooks/useDashboard.ts`, `src/components/AccountRow.tsx` (sparkline + prop type only), `src/components/AccountsTable.tsx` (thread `cycle`), `src/App.tsx` (thread `cycle`)

**Interfaces:**
- Consumes: Task 4 (`alignedSince`, `UNITS`, `WEEK_ALL`, `Metric`, limits).
- Produces:
  - `Backend.setAlwaysOnTop(flag: boolean): Promise<void>` on the interface, real and mock.
  - Mock commands `get_history { accountId, since, bucketMs, metric }` (validated like Rust, throws `{code:"out_of_range"}`) and `get_history_models { accountId }`.
  - `useDashboard()` returns `{ dashboard, history, now, cycle, error, refetch }`; `cycle` increments on mount and every `cycle:finished`.
  - `AccountsTable` and `AccountRow` accept `cycle: number`; `AccountsTable` passes it to every `AccountRow`; `AccountRow` declares it in `Props` but does not read it yet (Task 6 does).

- [ ] **Step 1: Backend seam**

`src/lib/backend.ts` (top of the file; the rest is unchanged):

```ts
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

/** The backend primitives the UI uses. Swappable for a browser mock. */
export interface Backend {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: () => void): Promise<() => void>;
  setAlwaysOnTop(flag: boolean): Promise<void>;
}

const real: Backend = {
  invoke: <T>(command: string, args?: Record<string, unknown>): Promise<T> =>
    tauriInvoke<T>(command, args),
  listen: async (event, handler) => {
    const off = await tauriListen(event, () => handler());
    return () => off();
  },
  setAlwaysOnTop: (flag) => getCurrentWindow().setAlwaysOnTop(flag),
};
```

- [ ] **Step 2: Mock backend**

In `src/lib/mockBackend.ts`:

- Replace the imports and constants at the top:

```ts
import type { Backend } from "./backend";
import type { Metric } from "./history";
import limits from "./historyLimits.json";
import type {
  Account,
  AppErrorShape,
  Dashboard,
  HistoryPoint,
  RawSnapshot,
  SnapshotDto,
  UserSettings,
} from "./types";

const DAY_MS = 24 * 60 * 60 * 1000;
const MIN_INTERVAL_SECS = 10;
const MAX_INTERVAL_SECS = 3600;
const MIN_TIMEOUT_SECS = 5;
const MAX_TIMEOUT_SECS = 120;
const RANGE_SLACK_MS = 300_000;
const MAX_LABEL_LEN = 64;
```

- `MockAccount` becomes `{ account: Account; latest: SnapshotDto | null; samples: Sample[] }` with

```ts
/** One poll, as the real `snapshots` row stores it (minus the noise). */
interface Sample { t: number; session: number; week_all: number; models: Array<{ label: string; pct: number }> }
```

- Delete `clampDays` **and** `optionalNumberArg` (the old `get_history` handler was its only caller; leaving it fails `noUnusedLocals`). Replace `buildHistory` with:

```ts
const HISTORY_START_HOUR = 9;
const HISTORY_PEAK_HOUR = 14;
const HISTORY_END_HOUR = 18;
const SAMPLE_GAP_MS = 2 * 60_000;

function clampPct(v: number): number {
  return Math.max(0, Math.min(100, Math.round(v)));
}

/**
 * Day index `29` is today. Each non-null day value seeds one sample every
 * two minutes from 09:00 to 18:00 local (roughly real poll density), so
 * every preset/unit combination has something to show. Week-all rises from
 * `v-8` at 09:00 to `v` at 14:00 then eases down to `v-3`; session is a
 * half-sine over the working day; each model sits at `v + offset`.
 */
function buildSamples(days: ReadonlyArray<number | null>, models: ReadonlyArray<{ label: string; offset: number }>): Sample[] {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const out: Sample[] = [];
  days.forEach((v, i) => {
    if (v === null) return;
    const date = new Date(today);
    date.setDate(date.getDate() - (days.length - 1 - i));
    const start = new Date(date);
    start.setHours(HISTORY_START_HOUR, 0, 0, 0);
    const end = new Date(date);
    end.setHours(HISTORY_END_HOUR, 0, 0, 0);
    const peak = new Date(date);
    peak.setHours(HISTORY_PEAK_HOUR, 0, 0, 0);
    for (let t = start.getTime(); t <= end.getTime(); t += SAMPLE_GAP_MS) {
      let delta: number;
      if (t <= peak.getTime()) {
        const f = (t - start.getTime()) / (peak.getTime() - start.getTime());
        delta = -8 + 8 * f;
      } else {
        const f = (t - peak.getTime()) / (end.getTime() - peak.getTime());
        delta = -3 * f * f;
      }
      const dayFraction = (t - start.getTime()) / (end.getTime() - start.getTime());
      out.push({
        t,
        week_all: clampPct(v + delta),
        session: clampPct(70 * Math.sin(Math.PI * dayFraction)),
        models: models.map((m) => ({ label: m.label, pct: clampPct(v + delta + m.offset) })),
      });
    }
  });
  return out;
}
```

- In `seedAccounts`, the three `history:` fields become `samples: buildSamples(claude3Days, [{ label: "Fable", offset: 1 }, { label: "Opus", offset: -20 }])`, `samples: buildSamples(claudeDays, [{ label: "Fable", offset: -30 }])`, `samples: buildSamples(claude2Days, [{ label: "Fable", offset: 1 }, { label: "Sonnet", offset: -40 }])`. `add_account` pushes `{ account, latest: null, samples: [] }`.

- Add these helpers above `createMockBackend`:

```ts
function isMetric(v: unknown): v is Metric {
  if (!isRecord(v)) return false;
  if (v.kind === "week_all" || v.kind === "session") return true;
  return v.kind === "model" && typeof v.label === "string";
}

function metricArg(args: Record<string, unknown>): Metric {
  const v = args.metric;
  if (!isMetric(v)) {
    throw { code: "internal", message: "mock: metric must be a HistoryMetric" } satisfies AppErrorShape;
  }
  return v;
}

function outOfRange(message: string): AppErrorShape {
  return { code: "out_of_range", message };
}

/** Mirrors commands::validate_history_request so the UI's error path is exercisable in a browser. */
function validateHistory(now: number, since: number, bucketMs: number, metric: Metric): void {
  if (bucketMs < limits.minBucketMs) throw outOfRange(`bucket_ms must be >= ${limits.minBucketMs}, got ${bucketMs}`);
  if (since > now) throw outOfRange("since must not be in the future");
  const range = now - since;
  if (range > limits.maxRangeMs + RANGE_SLACK_MS) throw outOfRange(`range must be <= ${limits.maxRangeMs} ms`);
  if (Math.ceil(range / bucketMs) > limits.maxBuckets) throw outOfRange(`request spans more than ${limits.maxBuckets} buckets`);
  if (metric.kind === "model") {
    const trimmed = metric.label.trim();
    if (trimmed.length === 0) throw outOfRange("model label must not be blank");
    if ([...trimmed].length > MAX_LABEL_LEN) throw outOfRange(`model label must be <= ${MAX_LABEL_LEN} characters`);
  }
}

function sampleValue(s: Sample, metric: Metric): number | null {
  switch (metric.kind) {
    case "week_all": return s.week_all;
    case "session": return s.session;
    case "model": return s.models.find((m) => m.label === metric.label)?.pct ?? null;
  }
}
```

- Replace the `get_history` handler and add `get_history_models`:

```ts
    get_history: (args) => {
      const accountId = stringArg(args, "accountId");
      const since = numberArg(args, "since");
      const bucketMs = numberArg(args, "bucketMs");
      const metric = metricArg(args);
      validateHistory(Date.now(), since, bucketMs, metric);
      const buckets = new Map<number, number>();
      for (const s of findAccount(accountId).samples) {
        if (s.t < since) continue;
        const v = sampleValue(s, metric);
        if (v === null) continue;
        const b = since + Math.floor((s.t - since) / bucketMs) * bucketMs;
        buckets.set(b, Math.max(buckets.get(b) ?? 0, v));
      }
      return [...buckets.entries()]
        .sort((a, b) => a[0] - b[0])
        .map(([t, pct]): HistoryPoint => ({ t, pct }));
    },

    get_history_models: (args) => {
      const accountId = stringArg(args, "accountId");
      const labels = new Set<string>();
      for (const s of findAccount(accountId).samples) for (const m of s.models) labels.add(m.label);
      return [...labels].sort();
    },
```

- Add to the returned object, after `listen`: `setAlwaysOnTop: async (flag) => { console.info("mock: setAlwaysOnTop", flag); },`.

- [ ] **Step 3: Dashboard hook**

In `src/hooks/useDashboard.ts`:

- Replace `import { HISTORY_DAYS } from "../lib/series";` with `import { UNITS, WEEK_ALL, alignedSince } from "../lib/history";`.
- Add `cycle: number;` to `UseDashboard`, and `const [cycle, setCycle] = useState(0);` after the `error` state.
- Replace `loadHistoryFor` and `loadWithHistory`:

```ts
  /** 7 days of hourly week-all maxima per account, for the row sparklines. */
  const loadHistoryFor = useCallback(async (accountIds: string[]): Promise<void> => {
    const now = Date.now();
    const since = alignedSince(now, "7d", "1h");
    try {
      const entries = await Promise.all(
        accountIds.map(async (id) => {
          const points = await backend().invoke<HistoryPoint[]>("get_history", {
            accountId: id,
            since,
            bucketMs: UNITS["1h"].ms,
            metric: WEEK_ALL,
          });
          return [id, points] as const;
        }),
      );
      setHistory(Object.fromEntries(entries));
    } catch (e) {
      setError(errorMessage(e));
    }
  }, []);

  /** Runs on mount and on every cycle:finished; `cycle` tells open drawers to refetch. */
  const loadWithHistory = useCallback(async (): Promise<void> => {
    const d = await load();
    if (d !== null) await loadHistoryFor(d.accounts.map((r) => r.account.id));
    setCycle((c) => c + 1);
  }, [load, loadHistoryFor]);
```

- Return `{ dashboard, history, now, cycle, error, refetch }`.

- [ ] **Step 4: Thread `cycle` and drop `last7`**

- `src/App.tsx`: `const { dashboard, history, now, cycle, error, refetch } = useDashboard();` and pass `cycle={cycle}` to `AccountsTable`.
- `src/components/AccountsTable.tsx`: add `cycle: number;` to `Props`, add `cycle` to the destructured parameter list, pass `cycle={cycle}` to each `AccountRow`.
- `src/components/AccountRow.tsx`: add `cycle: number;` to `Props` (do **not** add it to the `const { … } = props;` destructure yet — an unused local fails `noUnusedLocals`; Task 6 reads it); delete the `last7` function and its doc comment; the spark cell becomes `<Sparkline points={points} stroke={stroke} />`.

- [ ] **Step 5: Run all four gates**

Run: `npm test`, `npm run build`, `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
Expected: green. (`series.ts` still exports `HISTORY_DAYS` etc. for the old drawer; Task 6 removes them. Between this commit and Task 6 the old drawer still asks `dailyMax` for 30 days but only 7 days of points arrive, so its chart shows 23 empty days — a known, temporary gap the gates cannot see; Task 6 replaces that drawer in the very next commit. Do not "fix" it here by widening the sparkline request.)

- [ ] **Step 6: Commit**

```bash
git add src/lib/backend.ts src/lib/mockBackend.ts src/hooks/useDashboard.ts src/components/AccountRow.tsx src/components/AccountsTable.tsx src/App.tsx
git commit -m "feat(ui): metric-aware history requests, mock bucketing, cycle counter"
```

---

### Task 6: The history drawer — range, granularity, metric, hover

**Files:**
- Modify: `src/components/HistoryDrawer.tsx` (rewrite), `src/components/AccountRow.tsx` (drawer call), `src/lib/series.ts`, `src/lib/series.test.ts`, `src/styles.css`

**Interfaces:**
- Consumes: Task 4 (`history.ts`), Task 5 (`cycle`, mock commands), `polylineRuns`/`seriesDots`/`seriesStats`, `metricColor`.
- Produces: `HistoryDrawer` props `{ accountId: string; latest: SnapshotDto | null; cycle: number; onError: (m: string) => void; onCollapse: () => void }`. `series.ts` keeps only `polylineRuns`, `seriesDots`, `seriesStats`.

- [ ] **Step 1: Trim `series.ts` and its tests**

`src/lib/series.ts` becomes:

```ts
function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

/**
 * One `points` string per contiguous run of known values, so a gap in the
 * data breaks the line instead of connecting across missing slots. A run of
 * length 1 is emitted as a single point (harmless as a `<polyline>`; the
 * drawer draws a dot for it when dots are on).
 */
export function polylineRuns(
  vals: readonly (number | null)[],
  width: number,
  height: number,
): string[] {
  const n = vals.length;
  const point = (i: number, v: number): string => {
    const x = n === 1 ? 0 : (i / (n - 1)) * width;
    const y = height - (clamp(v, 0, 100) / 100) * height;
    return `${x.toFixed(2)},${y.toFixed(2)}`;
  };
  const runs: string[] = [];
  let current: string[] = [];
  vals.forEach((v, i) => {
    if (v === null) {
      if (current.length > 0) { runs.push(current.join(" ")); current = []; }
      return;
    }
    current.push(point(i, v));
  });
  if (current.length > 0) runs.push(current.join(" "));
  return runs;
}

export interface SeriesDot { index: number; value: number; leftPct: number }

export function seriesDots(vals: readonly (number | null)[]): SeriesDot[] {
  const n = vals.length;
  const out: SeriesDot[] = [];
  vals.forEach((v, i) => {
    if (v === null) return;
    out.push({ index: i, value: v, leftPct: n === 1 ? 0 : (i / (n - 1)) * 100 });
  });
  return out;
}

export interface SeriesStats { peak: number; avg: number; missing: number }

export function seriesStats(vals: readonly (number | null)[]): SeriesStats {
  const known = vals.filter((v): v is number => v !== null);
  const sum = known.reduce((a, b) => a + b, 0);
  return {
    peak: known.length ? Math.max(...known) : 0,
    avg: known.length ? Math.round(sum / known.length) : 0,
    missing: vals.length - known.length,
  };
}
```

`src/lib/series.test.ts` becomes:

```ts
import { describe, expect, it } from "vitest";
import { polylineRuns, seriesDots, seriesStats } from "./series";

describe("polylineRuns", () => {
  it("emits one run per contiguous span of known values, breaking across nulls", () => {
    expect(polylineRuns([0, null, 100], 100, 24)).toEqual(["0.00,24.00", "100.00,0.00"]);
  });
  it("emits a single-point run for an isolated known value amid gaps", () => {
    expect(polylineRuns([10, 20, null, 30], 100, 24)).toEqual([
      "0.00,21.60 33.33,19.20",
      "100.00,16.80",
    ]);
  });
  it("is empty for an empty series or a series with no known values", () => {
    expect(polylineRuns([], 100, 24)).toEqual([]);
    expect(polylineRuns([null, null], 100, 24)).toEqual([]);
  });
  it("clamps values into 0..100", () => {
    expect(polylineRuns([150, -10], 100, 100)).toEqual(["0.00,0.00 100.00,100.00"]);
  });
});

describe("seriesDots / seriesStats", () => {
  it("emits one dot per known value with its left percentage", () => {
    expect(seriesDots([10, null, 30])).toEqual([
      { index: 0, value: 10, leftPct: 0 },
      { index: 2, value: 30, leftPct: 100 },
    ]);
    expect(seriesDots([5])).toEqual([{ index: 0, value: 5, leftPct: 0 }]);
  });
  it("computes peak, rounded average and missing count", () => {
    expect(seriesStats([10, null, 31])).toEqual({ peak: 31, avg: 21, missing: 1 });
    expect(seriesStats([null])).toEqual({ peak: 0, avg: 0, missing: 1 });
  });
  it("is all zeros for an empty series", () => {
    expect(seriesStats([])).toEqual({ peak: 0, avg: 0, missing: 0 });
  });
});
```

Run `npm test -- src/lib/series.test.ts` — PASS. (`npm run build` fails until Step 2 because the old drawer imports the deleted helpers; that is expected mid-task.)

- [ ] **Step 2: Rewrite the drawer**

`src/components/HistoryDrawer.tsx`:

```tsx
import type { JSX, MouseEvent as ReactMouseEvent } from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { backend } from "../lib/backend";
import { errorMessage } from "../lib/errors";
import {
  type Metric, type PresetKey, type UnitKey,
  PRESETS, PRESET_KEYS, UNITS, UNIT_KEYS, WEEK_ALL,
  alignedSince, axisLabelsFor, bucketCount, bucketSeries, effectiveUnit,
  metricFromKey, metricKey, metricLabel, missingLabel, nearestKnownSlot, showMissing, slotLabel, unitAllowed,
} from "../lib/history";
import { polylineRuns, seriesDots, seriesStats } from "../lib/series";
import { metricColor } from "../lib/theme";
import type { HistoryPoint, SnapshotDto } from "../lib/types";

interface Props {
  accountId: string;
  latest: SnapshotDto | null;
  /** Bumped by useDashboard on every cycle:finished; a change refetches. */
  cycle: number;
  onError: (message: string) => void;
  onCollapse: () => void;
}

/** One resolved request. The chart renders ONLY from this, never from the pickers. */
interface Result { points: HistoryPoint[]; since: number; unit: UnitKey; count: number; fetchNow: number }
interface Tip { left: string; bottom: string; edge: "left" | "mid" | "right"; text: string }

const TIP_TRANSFORM: Record<Tip["edge"], string> = {
  right: "translate(-100%, calc(100% + 14px))",
  left: "translate(0, calc(100% + 14px))",
  mid: "translate(-50%, calc(100% + 14px))",
};
/** Dots are decorative; above this many slots they are skipped (hover still works). */
const MAX_DOTS = 200;
const AXIS_LABELS = 4;

function latestValue(latest: SnapshotDto | null, metric: Metric): number {
  if (latest === null) return 0;
  switch (metric.kind) {
    case "week_all": return latest.week_all?.pct ?? 0;
    case "session": return latest.session?.pct ?? 0;
    case "model": return latest.week_models.find((m) => m.label === metric.label)?.pct ?? 0;
  }
}

export function HistoryDrawer({ accountId, latest, cycle, onError, onCollapse }: Props): JSX.Element {
  const [preset, setPreset] = useState<PresetKey>("30d");
  const [unitOverride, setUnitOverride] = useState<UnitKey | null>(null);
  const [metric, setMetric] = useState<Metric>(WEEK_ALL);
  const [models, setModels] = useState<string[]>([]);
  const [result, setResult] = useState<Result | null>(null);
  const [loading, setLoading] = useState(false);
  const [tip, setTip] = useState<Tip | null>(null);
  // Only the most recently started request may write `result`.
  const seq = useRef(0);
  const onErrorRef = useRef(onError);
  useEffect(() => { onErrorRef.current = onError; }, [onError]);

  useEffect(() => {
    let cancelled = false;
    const load = async (): Promise<void> => {
      try {
        const labels = await backend().invoke<string[]>("get_history_models", { accountId });
        if (!cancelled) setModels(labels);
      } catch (e) {
        if (!cancelled) onErrorRef.current(errorMessage(e));
      }
    };
    void load();
    return () => { cancelled = true; };
  }, [accountId]);

  const unit = effectiveUnit(preset, unitOverride);

  useEffect(() => {
    const id = ++seq.current;
    const fetchNow = Date.now();
    const since = alignedSince(fetchNow, preset, unit);
    const count = bucketCount(since, fetchNow, unit);
    setLoading(true);
    const load = async (): Promise<void> => {
      try {
        const points = await backend().invoke<HistoryPoint[]>("get_history", {
          accountId, since, bucketMs: UNITS[unit].ms, metric,
        });
        if (id === seq.current) setResult({ points, since, unit, count, fetchNow });
      } catch (e) {
        if (id === seq.current) onErrorRef.current(errorMessage(e));
      } finally {
        if (id === seq.current) setLoading(false);
      }
    };
    void load();
  }, [accountId, preset, unit, metric, cycle]);

  const choosePreset = (p: PresetKey): void => {
    setPreset(p);
    if (unitOverride !== null && !unitAllowed(p, unitOverride)) setUnitOverride(null);
  };

  // Derived once per result, not per mouse move (a 720-slot series would
  // otherwise be re-bucketed and re-stringified on every pointer event).
  const { vals, stats, lines, dots } = useMemo(() => {
    const v: Array<number | null> = result === null ? [] : bucketSeries(result.points, result.since, result.unit, result.count);
    return {
      vals: v,
      stats: seriesStats(v),
      lines: polylineRuns(v, 100, 100),
      dots: v.length <= MAX_DOTS ? seriesDots(v) : [],
    };
  }, [result]);
  const stroke = metricColor(latestValue(latest, metric));
  const shownUnit = result?.unit ?? unit;
  const axis = result === null ? [] : axisLabelsFor(result.since, result.unit, result.count, AXIS_LABELS);
  const selectedKey = metricKey(metric);
  const options = metric.kind === "model" && !models.includes(metric.label) ? [...models, metric.label] : models;

  const onPlotMove = (e: ReactMouseEvent<HTMLDivElement>): void => {
    if (result === null) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const fraction = rect.width > 0 ? (e.clientX - rect.left) / rect.width : 0;
    const idx = nearestKnownSlot(vals, fraction);
    const value = idx === null ? null : vals[idx];
    if (idx === null || value === null || value === undefined) { setTip(null); return; }
    const leftPct = vals.length === 1 ? 0 : (idx / (vals.length - 1)) * 100;
    setTip({
      left: `${leftPct.toFixed(2)}%`,
      bottom: `${value}%`,
      edge: leftPct > 78 ? "right" : leftPct < 22 ? "left" : "mid",
      text: `${slotLabel(result.since, result.unit, idx)} · ${value}%`,
    });
  };

  return (
    <div className="drawer">
      <div className="drawer-head">
        <span className="drawer-label">
          Last {PRESETS[preset].label} · {metricLabel(metric)} · {shownUnit}{loading ? " · loading" : ""}
        </span>
        <div className="drawer-stats">
          <span>peak {stats.peak}%</span>
          <span>avg {stats.avg}%</span>
          {showMissing(shownUnit) && <span>missing {missingLabel(stats.missing, shownUnit)}</span>}
          <button type="button" className="btn-link" onClick={onCollapse}>collapse</button>
        </div>
      </div>
      <div className="drawer-controls">
        <div className="drawer-group" role="group" aria-label="Range">
          {PRESET_KEYS.map((p) => (
            <button key={p} type="button" className={`btn btn-sm${p === preset ? " btn-edit-on" : ""}`} onClick={() => choosePreset(p)}>{p}</button>
          ))}
        </div>
        <div className="drawer-group" role="group" aria-label="Granularity">
          <button type="button" className={`btn btn-sm${unitOverride === null ? " btn-edit-on" : ""}`} onClick={() => setUnitOverride(null)}>auto</button>
          {UNIT_KEYS.map((u) => (
            <button key={u} type="button" disabled={!unitAllowed(preset, u)}
              className={`btn btn-sm${unitOverride === u ? " btn-edit-on" : ""}`} onClick={() => setUnitOverride(u)}>{u}</button>
          ))}
        </div>
        <select className="input input-mono drawer-metric" aria-label="Metric" value={selectedKey}
          onChange={(e) => setMetric(metricFromKey(e.target.value))}>
          <option value="week_all">weekly limit</option>
          <option value="session">session</option>
          {options.map((label) => <option key={label} value={`model:${label}`}>{label}</option>)}
        </select>
      </div>
      <div className="chart">
        <svg viewBox="0 0 100 100" preserveAspectRatio="none" role="img" aria-label={`${metricLabel(metric)}, last ${PRESETS[preset].label}`}>
          {[0, 50, 100].map((y) => <line key={y} x1={0} y1={y} x2={100} y2={y} stroke="#1e252a" strokeWidth={1} vectorEffect="non-scaling-stroke" />)}
          {lines.map((points, i) => (
            <polyline key={i} points={points} fill="none" stroke={stroke} strokeWidth={1.8} vectorEffect="non-scaling-stroke" strokeLinejoin="round" strokeLinecap="round" />
          ))}
        </svg>
        <div className="chart-dots" onMouseMove={onPlotMove} onMouseLeave={() => setTip(null)}>
          {dots.map((d) => (
            <div key={d.index} className="dot" style={{ left: `${d.leftPct.toFixed(2)}%`, bottom: `${d.value}%` }} />
          ))}
          {tip !== null && (
            <div className="tip" style={{ left: tip.left, bottom: tip.bottom, transform: TIP_TRANSFORM[tip.edge] }}>{tip.text}</div>
          )}
        </div>
        <span className="chart-y chart-y-top">100%</span>
        <span className="chart-y chart-y-bottom">0%</span>
      </div>
      <div className="chart-axis">
        {axis.map((t, i) => <span key={i}>{t}</span>)}
      </div>
    </div>
  );
}
```

- [ ] **Step 3: Wire it from the row and style the controls**

In `src/components/AccountRow.tsx`: add `cycle` to the `const { … } = props;` destructure, and the drawer element becomes:

```tsx
          <HistoryDrawer accountId={row.account.id} latest={row.latest} cycle={cycle} onError={onError} onCollapse={onToggleChart} />
```

In `src/styles.css`, under `/* ---- drawers ---- */`:

- change `.chart-dots { position: absolute; inset: 14px 16px; pointer-events: none; }` to `.chart-dots { position: absolute; inset: 14px 16px; pointer-events: auto; }`
- in the `.dot { … }` rule change `pointer-events: auto;` to `pointer-events: none;`, and delete the `.dot:hover { … }` rule
- add:

```css
.drawer-controls { display: flex; align-items: center; gap: 14px; flex-wrap: wrap; }
.drawer-group { display: flex; gap: 4px; }
.drawer-metric { padding: 5px 8px; font-size: 11.5px; }
```

- [ ] **Step 4: Run all four gates**

Run: `npm test`, `npm run build`, `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
Expected: all green. `grep -rn "HISTORY_DAYS\|dailyMax\|dayLabel\|axisLabels\b\|last7" src` returns nothing.

- [ ] **Step 5: Visual check against the mock**

Start `$env:VITE_MOCK_BACKEND='1'; npm run dev` (background). With the Playwright MCP: navigate to `http://localhost:1420`, resize to 980×700, click the first row's 7-day sparkline, then screenshot the drawer at: default (30d/1d, weekly limit); click `1h` (unit auto = 1m, `1h`/`1d` buttons disabled, no "missing" stat); `24h` then unit `15m`; `7d` (dots visible, dates on the axis); pick the `Opus` model in the select; hover the plot and confirm a tooltip like `Sep 16 14:30 · 47%`. Save the screenshots to the scratchpad directory. Stop the dev server.

- [ ] **Step 6: Commit**

```bash
git add src/components/HistoryDrawer.tsx src/components/AccountRow.tsx src/lib/series.ts src/lib/series.test.ts src/styles.css
git commit -m "feat(ui): history drawer range, granularity and metric controls"
```

---

### Task 7: Column and prefs helpers for hidden columns and always-on-top (pure)

**Files:**
- Modify: `src/lib/columns.ts`, `src/lib/prefs.ts`
- Test: `src/lib/columns.test.ts`, `src/lib/prefs.test.ts`

**Interfaces:**
- Consumes: `moveItem` from `src/lib/reorder.ts`.
- Produces (`columns.ts`): `ALWAYS_VISIBLE: readonly ColumnKey[]`, `GRID_GAP = 10`, `visibleColumns(order, hidden): ColumnKey[]`, `moveVisible(order, hidden, from, to): ColumnKey[]`, `normalizeHiddenColumns(v: unknown): ColumnKey[] | null`, `gridMinWidth(order): number`.
- Produces (`prefs.ts`): `Prefs { font; size; columnOrder; hiddenColumns: ColumnKey[]; alwaysOnTop: boolean }`, defaults `[]` and `false`.

- [ ] **Step 1: Write the failing tests**

Append to `src/lib/columns.test.ts` (and extend its import to `import { COLUMNS, DEFAULT_ORDER, GRID_GAP, gridMinWidth, gridTemplate, moveVisible, normalizeColumnOrder, normalizeHiddenColumns, visibleColumns } from "./columns";`):

```ts
describe("visibleColumns", () => {
  it("filters hidden keys and never drops account", () => {
    expect(visibleColumns(DEFAULT_ORDER, ["session", "updated"])).toEqual(["account", "week", "model", "spark"]);
    expect(visibleColumns(DEFAULT_ORDER, ["account"])).toEqual([...DEFAULT_ORDER]);
    expect(visibleColumns(DEFAULT_ORDER, [])).toEqual([...DEFAULT_ORDER]);
  });
});

describe("moveVisible", () => {
  it("equals a plain move when nothing is hidden", () => {
    expect(moveVisible(DEFAULT_ORDER, [], 4, 1)).toEqual(["account", "spark", "session", "week", "model", "updated"]);
  });
  it("moves only the dragged key and leaves hidden keys where they sit", () => {
    // full: account session week model spark updated; hidden: model, updated
    // visible: account session week spark; drag spark (3) to 1
    expect(moveVisible(DEFAULT_ORDER, ["model", "updated"], 3, 1)).toEqual(["account", "spark", "session", "week", "model", "updated"]);
  });
  it("is well defined when a hidden key precedes visible index 0", () => {
    const order = ["session", "account", "week", "spark", "model", "updated"] as const;
    // hidden: session; visible: account week spark model updated; drag week (1) to 0
    expect(moveVisible(order, ["session"], 1, 0)).toEqual(["session", "week", "account", "spark", "model", "updated"]);
  });
  it("a drag made under the narrow auto-hidden set, then widened, shows only the dragged key moved", () => {
    const narrowHidden = ["updated", "model"] as const;
    const after = moveVisible(DEFAULT_ORDER, narrowHidden, 3, 0); // spark to the front
    expect(after).toEqual(["spark", "account", "session", "week", "model", "updated"]);
    expect(visibleColumns(after, [])).toEqual(after);
  });
  it("returns the order unchanged for out-of-range indices", () => {
    expect(moveVisible(DEFAULT_ORDER, ["model"], 5, 0)).toEqual([...DEFAULT_ORDER]);
    expect(moveVisible(DEFAULT_ORDER, [], -1, 0)).toEqual([...DEFAULT_ORDER]);
  });
});

describe("normalizeHiddenColumns", () => {
  it("keeps known keys except account, drops unknowns and duplicates, preserves order", () => {
    expect(normalizeHiddenColumns(["updated", "account", "bogus", "session", "updated"])).toEqual(["updated", "session"]);
    expect(normalizeHiddenColumns([])).toEqual([]);
  });
  it("returns null for a non-array or non-string elements", () => {
    expect(normalizeHiddenColumns(null)).toBeNull();
    expect(normalizeHiddenColumns("session")).toBeNull();
    expect(normalizeHiddenColumns([1])).toBeNull();
  });
});

describe("gridMinWidth", () => {
  it("sums track minimums plus the gaps between tracks", () => {
    expect(GRID_GAP).toBe(10);
    // 26 + 150 + 3*86 + 76 + 82 + 62 = 654 tracks, 7 gaps
    expect(gridMinWidth(DEFAULT_ORDER)).toBe(654 + 7 * GRID_GAP);
    // 26 + 150 + 86 + 86 + 76 + 62 = 486 tracks, 5 gaps
    expect(gridMinWidth(["account", "session", "week", "spark"])).toBe(486 + 5 * GRID_GAP);
    expect(COLUMNS.account.width).toBe("minmax(150px,1fr)");
  });
});
```

In `src/lib/prefs.test.ts`:

- `keeps valid fields and defaults invalid ones independently` expects `{ font: "plex", size: "md", columnOrder: [...DEFAULT_ORDER], hiddenColumns: [], alwaysOnTop: false }`.
- `accepts a full valid record` becomes:

```ts
  it("accepts a full valid record", () => {
    const order = [...DEFAULT_ORDER].reverse();
    const raw = JSON.stringify({ font: "jetbrains", size: "xl", columnOrder: order, hiddenColumns: ["session"], alwaysOnTop: true });
    expect(parsePrefs(raw)).toEqual({ font: "jetbrains", size: "xl", columnOrder: order, hiddenColumns: ["session"], alwaysOnTop: true });
  });
```

- The round-trip test's `prefs` literal becomes `{ font: "plex" as const, size: "lg" as const, columnOrder: [...DEFAULT_ORDER], hiddenColumns: ["updated" as const], alwaysOnTop: true }`.
- Add inside `describe("parsePrefs")`:

```ts
  it("defaults hiddenColumns and alwaysOnTop independently of each other", () => {
    const a = parsePrefs(JSON.stringify({ hiddenColumns: ["model", "account", "nope"], alwaysOnTop: "yes" }));
    expect(a.hiddenColumns).toEqual(["model"]);
    expect(a.alwaysOnTop).toBe(false);
    const b = parsePrefs(JSON.stringify({ hiddenColumns: "model", alwaysOnTop: true }));
    expect(b.hiddenColumns).toEqual([]);
    expect(b.alwaysOnTop).toBe(true);
  });
  it("warns when a stored hiddenColumns value is unusable", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    parsePrefs(JSON.stringify({ hiddenColumns: 42 }));
    expect(warn).toHaveBeenCalledTimes(1);
    warn.mockRestore();
  });
  it("never aliases the default hiddenColumns array", () => {
    const a = parsePrefs(null);
    a.hiddenColumns.push("session");
    expect(parsePrefs(null).hiddenColumns).toEqual([]);
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `npm test -- src/lib/columns.test.ts src/lib/prefs.test.ts`
Expected: FAIL (missing exports; records do not match).

- [ ] **Step 3: Implement**

Append to `src/lib/columns.ts` (add `import { moveItem } from "./reorder";` at the top):

```ts
/** Columns that can never be hidden. */
export const ALWAYS_VISIBLE: readonly ColumnKey[] = ["account"];

/** `.row-grid` / `.thead` gap in styles.css; gridMinWidth depends on it. */
export const GRID_GAP = 10;

export function visibleColumns(order: readonly ColumnKey[], hidden: readonly ColumnKey[]): ColumnKey[] {
  return order.filter((k) => ALWAYS_VISIBLE.includes(k) || !hidden.includes(k));
}

/**
 * `from`/`to` index the VISIBLE list. Only the dragged key moves, to the
 * full-order position of the key currently at visible index `to`; hidden
 * keys stay exactly where they are, so a drag made while columns are hidden
 * never rearranges columns the user could not see. With nothing hidden this
 * is `moveItem`. Out-of-range indices return a copy of `order`.
 */
export function moveVisible(
  order: readonly ColumnKey[],
  hidden: readonly ColumnKey[],
  from: number,
  to: number,
): ColumnKey[] {
  const visible = visibleColumns(order, hidden);
  const fromKey = visible[from];
  const toKey = visible[to];
  if (fromKey === undefined || toKey === undefined) return [...order];
  return moveItem(order, order.indexOf(fromKey), order.indexOf(toKey));
}

/**
 * Known keys minus `account`, de-duplicated, in the stored order. Null when
 * `v` is not an array of strings at all (the caller warns and defaults).
 */
export function normalizeHiddenColumns(v: unknown): ColumnKey[] | null {
  if (!Array.isArray(v)) return null;
  const out: ColumnKey[] = [];
  for (const item of v) {
    if (typeof item !== "string") return null;
    if (isColumnKey(item) && !ALWAYS_VISIBLE.includes(item) && !out.includes(item)) out.push(item);
  }
  return out;
}

/** Minimum px the grid needs: every track's px floor plus the gaps between tracks. */
export function gridMinWidth(order: readonly ColumnKey[]): number {
  const px = (width: string): number => {
    const m = /(\d+(?:\.\d+)?)px/.exec(width);
    return m === null ? 0 : Number(m[1]);
  };
  const tracks = [LEAD_WIDTH, ...order.map((k) => COLUMNS[k].width), TRAIL_WIDTH];
  return tracks.reduce((sum, w) => sum + px(w), 0) + GRID_GAP * (tracks.length - 1);
}
```

`src/lib/prefs.ts`:

```ts
import { type ColumnKey, DEFAULT_ORDER, normalizeColumnOrder, normalizeHiddenColumns } from "./columns";
import { type FontKey, type SizeKey, isFontKey, isSizeKey } from "./theme";

export interface Prefs {
  font: FontKey;
  size: SizeKey;
  columnOrder: ColumnKey[];
  /** Columns the user hid in Settings; "account" is never in here. */
  hiddenColumns: ColumnKey[];
  alwaysOnTop: boolean;
}

export const PREFS_KEY = "usage-tracker.prefs.v1";

export const DEFAULT_PREFS: Readonly<Prefs> = {
  font: "system",
  size: "md",
  columnOrder: [...DEFAULT_ORDER],
  hiddenColumns: [],
  alwaysOnTop: false,
};

/** The subset of the Web Storage API the app touches; injectable for tests. */
export interface PrefsStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function fresh(): Prefs {
  return {
    font: DEFAULT_PREFS.font,
    size: DEFAULT_PREFS.size,
    columnOrder: [...DEFAULT_ORDER],
    hiddenColumns: [],
    alwaysOnTop: DEFAULT_PREFS.alwaysOnTop,
  };
}

/** Per-field validation: one bad field never discards the others. */
export function parsePrefs(raw: string | null): Prefs {
  const out = fresh();
  if (raw === null) return out;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return out;
  }
  if (typeof parsed !== "object" || parsed === null) return out;
  const rec = parsed as Record<string, unknown>;
  if (isFontKey(rec.font)) out.font = rec.font;
  if (isSizeKey(rec.size)) out.size = rec.size;
  const order = normalizeColumnOrder(rec.columnOrder);
  if (order !== null) out.columnOrder = order;
  if (Array.isArray(rec.columnOrder) && JSON.stringify(order) !== JSON.stringify(rec.columnOrder)) {
    console.warn("prefs: stored column order was invalid or out of date; normalizing", rec.columnOrder);
  }
  const hidden = normalizeHiddenColumns(rec.hiddenColumns);
  if (hidden !== null) out.hiddenColumns = hidden;
  else if (rec.hiddenColumns !== undefined) {
    console.warn("prefs: stored hiddenColumns was unusable; showing every column", rec.hiddenColumns);
  }
  if (typeof rec.alwaysOnTop === "boolean") out.alwaysOnTop = rec.alwaysOnTop;
  return out;
}

export function loadPrefs(store: PrefsStore | null): Prefs {
  if (store === null) return fresh();
  try {
    return parsePrefs(store.getItem(PREFS_KEY));
  } catch (e) {
    console.warn("prefs: could not read storage, using defaults", e);
    return fresh();
  }
}

export function savePrefs(store: PrefsStore | null, prefs: Prefs): void {
  if (store === null) return;
  try {
    store.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch (e) {
    console.warn("prefs: could not write storage", e);
  }
}
```

- [ ] **Step 4: Run all four gates**

Run: `npm test`, `npm run build`, `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
Expected: green (nothing consumes the new fields yet).

- [ ] **Step 5: Commit**

```bash
git add src/lib/columns.ts src/lib/columns.test.ts src/lib/prefs.ts src/lib/prefs.test.ts
git commit -m "feat(ui): hidden-column and always-on-top prefs with pure helpers"
```

---

### Task 8: Layout and gauge helpers (pure)

**Files:**
- Create: `src/lib/layout.ts`, `src/lib/gauge.ts`
- Test: `src/lib/layout.test.ts`, `src/lib/gauge.test.ts`

**Interfaces:**
- Consumes: `toLocal` (`drag.ts`), `ColumnKey`, `DEFAULT_ORDER`, `visibleColumns`, `gridMinWidth` (`columns.ts`).
- Produces: `type Layout = "full" | "narrow" | "cards"`, `BREAKPOINTS = { narrow: 820, cards: 640 }`, `SHELL_PADDING = 95`, `layoutFor(viewportWidthPx, zoom): Layout`, `autoHiddenColumns(layout): readonly ColumnKey[]`; `RING = { size: 44, stroke: 5, radius: 19.5 }`, `ringDash(pct: number | null, radius: number): { circumference: number; offset: number }`.

- [ ] **Step 1: Write the failing tests**

`src/lib/layout.test.ts` (the zoom cases: 1001/1.22 = 820.49, 1000/1.22 = 819.67, 781/1.22 = 640.16, 780/1.22 = 639.34):

```ts
import { describe, expect, it } from "vitest";
import { DEFAULT_ORDER, gridMinWidth, visibleColumns } from "./columns";
import { BREAKPOINTS, SHELL_PADDING, autoHiddenColumns, layoutFor } from "./layout";

describe("layoutFor", () => {
  it("switches at the breakpoints, measured in local px", () => {
    expect(BREAKPOINTS).toEqual({ narrow: 820, cards: 640 });
    expect(layoutFor(820, 1)).toBe("full");
    expect(layoutFor(819, 1)).toBe("narrow");
    expect(layoutFor(640, 1)).toBe("narrow");
    expect(layoutFor(639, 1)).toBe("cards");
  });
  it("divides the viewport width by the text-size zoom", () => {
    expect(layoutFor(1001, 1.22)).toBe("full");
    expect(layoutFor(1000, 1.22)).toBe("narrow");
    expect(layoutFor(781, 1.22)).toBe("narrow");
    expect(layoutFor(780, 1.22)).toBe("cards");
    expect(layoutFor(700, 0)).toBe("narrow"); // a bad zoom is treated as 1
  });
});

describe("autoHiddenColumns", () => {
  it("hides updated and model only in the narrow layout", () => {
    expect(autoHiddenColumns("full")).toEqual([]);
    expect(autoHiddenColumns("narrow")).toEqual(["updated", "model"]);
    expect(autoHiddenColumns("cards")).toEqual([]);
  });
});

describe("the table fits its band", () => {
  it("full table fits above the narrow breakpoint and the narrow table above the cards breakpoint", () => {
    expect(SHELL_PADDING).toBe(95);
    expect(gridMinWidth(DEFAULT_ORDER) + SHELL_PADDING).toBeLessThanOrEqual(BREAKPOINTS.narrow);
    const narrow = visibleColumns(DEFAULT_ORDER, autoHiddenColumns("narrow"));
    expect(gridMinWidth(narrow) + SHELL_PADDING).toBeLessThanOrEqual(BREAKPOINTS.cards);
  });
});
```

`src/lib/gauge.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { RING, ringDash } from "./gauge";

describe("ringDash", () => {
  const c = 2 * Math.PI * RING.radius;
  it("hides the arc for null, shows it in proportion, and clamps at 100", () => {
    expect(ringDash(null, RING.radius)).toEqual({ circumference: c, offset: c });
    expect(ringDash(0, RING.radius).offset).toBeCloseTo(c);
    expect(ringDash(50, RING.radius).offset).toBeCloseTo(c / 2);
    expect(ringDash(100, RING.radius).offset).toBeCloseTo(0);
    expect(ringDash(140, RING.radius).offset).toBeCloseTo(0);
    expect(ringDash(-5, RING.radius).offset).toBeCloseTo(c);
  });
  it("ring geometry fits the 44px box with a 5px stroke", () => {
    expect(RING).toEqual({ size: 44, stroke: 5, radius: 19.5 });
    expect(RING.radius + RING.stroke / 2).toBeLessThanOrEqual(RING.size / 2);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npm test -- src/lib/layout.test.ts src/lib/gauge.test.ts`
Expected: FAIL — modules not found.

- [ ] **Step 3: Implement**

`src/lib/layout.ts`:

```ts
import type { ColumnKey } from "./columns";
import { toLocal } from "./drag";

export type Layout = "full" | "narrow" | "cards";

/** Local (unzoomed) px. See spec §5.2 for the arithmetic behind each number. */
export const BREAKPOINTS = { narrow: 820, cards: 640 } as const;

/**
 * Horizontal px around the grid tracks that the table cannot use:
 * `.app` padding 2×22 + `.panel` border 2×1 + `.row-grid` padding 2×16
 * + 17 for WebView2's classic vertical scrollbar (window.innerWidth
 * includes it). Keep the three CSS terms in step with styles.css.
 */
export const SHELL_PADDING = 95;

export function layoutFor(viewportWidthPx: number, zoom: number): Layout {
  const w = toLocal(viewportWidthPx, zoom);
  if (w < BREAKPOINTS.cards) return "cards";
  if (w < BREAKPOINTS.narrow) return "narrow";
  return "full";
}

const NARROW_HIDDEN: readonly ColumnKey[] = ["updated", "model"];

/** Columns the layout hides on top of the user's own hidden set. */
export function autoHiddenColumns(layout: Layout): readonly ColumnKey[] {
  return layout === "narrow" ? NARROW_HIDDEN : [];
}
```

`src/lib/gauge.ts`:

```ts
/** Ring gauge geometry (spec §5.4): 44px box, 5px stroke, arc from 12 o'clock. */
export const RING = { size: 44, stroke: 5, radius: 19.5 } as const;

export function ringDash(pct: number | null, radius: number): { circumference: number; offset: number } {
  const circumference = 2 * Math.PI * radius;
  if (pct === null) return { circumference, offset: circumference };
  const clamped = Math.max(0, Math.min(100, pct));
  return { circumference, offset: circumference * (1 - clamped / 100) };
}
```

- [ ] **Step 4: Run all four gates**

Run: `npm test`, `npm run build`, `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add src/lib/layout.ts src/lib/layout.test.ts src/lib/gauge.ts src/lib/gauge.test.ts
git commit -m "feat(ui): layout breakpoints and ring gauge geometry"
```

---

### Task 9: Hideable columns in the app, Settings "Columns" section

**Files:**
- Create: `src/hooks/useViewport.ts`
- Modify: `src/App.tsx`, `src/components/AccountsTable.tsx`, `src/components/Settings.tsx`, `src/styles.css`

**Interfaces:**
- Consumes: Task 7 (`visibleColumns`, `moveVisible`, `Prefs.hiddenColumns`), Task 8 (`layoutFor`, `autoHiddenColumns`).
- Produces:
  - `useViewport(): number` — current `window.innerWidth`.
  - `AccountsTable` prop `onColumnMove: (from: number, to: number) => void` replaces `onColumnOrder`.
  - `Settings` props gain `layout: Layout`.
  - `App` computes `layout`, `effectiveHidden`, `visible`; the table renders `visible`.

- [ ] **Step 1: Viewport hook**

`src/hooks/useViewport.ts`:

```ts
import { useEffect, useState } from "react";

function currentWidth(): number {
  return typeof window === "undefined" ? 0 : window.innerWidth;
}

/**
 * `window.innerWidth`, re-read whenever the document element resizes.
 * innerWidth includes the vertical scrollbar, so a list that grows tall
 * enough to scroll cannot flip the layout back and forth at a breakpoint.
 */
export function useViewport(): number {
  const [width, setWidth] = useState<number>(currentWidth);
  useEffect(() => {
    if (typeof ResizeObserver === "undefined") {
      console.warn("layout: ResizeObserver unavailable; viewport width will not update");
      return undefined;
    }
    const observer = new ResizeObserver(() => setWidth(currentWidth()));
    observer.observe(document.documentElement);
    return () => observer.disconnect();
  }, []);
  return width;
}
```

- [ ] **Step 2: App wiring**

In `src/App.tsx`:

- Add imports: `import { useViewport } from "./hooks/useViewport";`, `import { moveVisible, visibleColumns } from "./lib/columns";`, `import { autoHiddenColumns, layoutFor } from "./lib/layout";`.
- After `const { prefs, update } = usePrefs();` add:

```ts
  const viewportWidth = useViewport();
  const zoom = SIZES[prefs.size].zoom;
  const layout = layoutFor(viewportWidth, zoom);
  const effectiveHidden = [...prefs.hiddenColumns, ...autoHiddenColumns(layout)];
  const visible = visibleColumns(prefs.columnOrder, effectiveHidden);
```

- In `shellStyle` use `zoom,` instead of `zoom: SIZES[prefs.size].zoom,`.
- The `AccountsTable` element becomes:

```tsx
        <AccountsTable
          rows={dashboard.accounts}
          history={history}
          now={now}
          cycle={cycle}
          zoom={zoom}
          columnOrder={visible}
          onColumnMove={(from, to) => update({ columnOrder: moveVisible(prefs.columnOrder, effectiveHidden, from, to) })}
          onChanged={refetch}
          onError={showError}
          onShowFailure={(id) => setFailureId(id)}
        />
```

- Pass `layout={layout}` to `Settings`.

- [ ] **Step 3: Table drop → index pair**

In `src/components/AccountsTable.tsx`:

- `Props`: replace `onColumnOrder: (order: ColumnKey[]) => void;` with `onColumnMove: (from: number, to: number) => void;`. Destructure `onColumnMove` instead of `onColumnOrder`.
- Replace `const onColumnOrderRef = useRef(onColumnOrder);` and its effect with `const onColumnMoveRef = useRef(onColumnMove);` and `useEffect(() => { onColumnMoveRef.current = onColumnMove; }, [onColumnMove]);`.
- Delete `const columnOrderRef = useRef(columnOrder);` and its `useEffect(() => { columnOrderRef.current = columnOrder; }, [columnOrder]);`.
- In `colUp`: replace `onColumnOrderRef.current(moveItem(columnOrderRef.current, c.index, c.target));` with `onColumnMoveRef.current(c.index, c.target);`.
- `moveItem` is still used by row drags; keep the import. `type ColumnKey` is still used by the `columnOrder` prop; keep it.

- [ ] **Step 4: Settings "Columns" section**

In `src/components/Settings.tsx`:

- Imports: `import { ALWAYS_VISIBLE, COLUMNS, type ColumnKey, DEFAULT_ORDER } from "../lib/columns";` and `import { type Layout, BREAKPOINTS, autoHiddenColumns } from "../lib/layout";`.
- `Props` gains `layout: Layout;`; destructure it.
- After the "Text size" section and before the first `<div className="divider" />`, add:

```tsx
      <div className="section">
        <div className="section-label">Columns</div>
        <div className="choices">
          {DEFAULT_ORDER.map((key) => {
            const pinned = ALWAYS_VISIBLE.includes(key);
            const autoHidden = autoHiddenColumns(layout).includes(key);
            const shown = !prefs.hiddenColumns.includes(key);
            const toggle = (): void => {
              const next: ColumnKey[] = shown
                ? [...prefs.hiddenColumns, key]
                : prefs.hiddenColumns.filter((k) => k !== key);
              onPrefs({ hiddenColumns: next });
            };
            return (
              <button
                key={key}
                type="button"
                className={"choice choice-size" + (shown ? " choice-on" : "")}
                disabled={pinned || autoHidden}
                title={pinned ? "Account is always shown" : undefined}
                aria-pressed={shown}
                onClick={toggle}
              >
                {COLUMNS[key].label}
              </button>
            );
          })}
        </div>
        {autoHiddenColumns(layout).length > 0 && (
          <span className="hint">
            {autoHiddenColumns(layout).map((k) => COLUMNS[k].label).join(" and ")} are hidden while the window is narrower than {BREAKPOINTS.narrow} px.
          </span>
        )}
      </div>
```

- `src/styles.css`: add `.choice:disabled { opacity: 0.5; cursor: default; }` after the `.choice-on` rule.

- [ ] **Step 5: Run all four gates, then a visual check**

Run: `npm test`, `npm run build`, `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`.

Visual (mock + Playwright): at 980 px open Settings, click "Session" off → the table loses the Session column and the header strip has five columns; drag the "7 days" header before "Week (all)" → after a page reload the order persists and `localStorage["usage-tracker.prefs.v1"]` shows `hiddenColumns: ["session"]` plus the moved order; click "Session" on → it comes back in its original place. Screenshots to the scratchpad.

- [ ] **Step 6: Commit**

```bash
git add src/App.tsx src/components/AccountsTable.tsx src/components/Settings.tsx src/hooks/useViewport.ts src/styles.css
git commit -m "feat(ui): hideable columns with a Columns section in Settings"
```

---

### Task 10: Compact view — cards, rings, header, minimum window

**Files:**
- Create: `src/components/StatusPill.tsx`, `src/components/Ring.tsx`, `src/components/AccountCard.tsx`
- Modify: `src/components/AccountRow.tsx` (use `StatusPill`), `src/components/Header.tsx`, `src/App.tsx`, `src/lib/present.ts`, `src/lib/present.test.ts`, `src/styles.css`, `src-tauri/tauri.conf.json`

**Interfaces:**
- Consumes: Task 8 (`RING`, `ringDash`, `Layout`), Task 9 (`layout` in `App`).
- Produces: `StatusPill({ pill, onShowFailure })`, `Ring({ pct, label, title? })`, `AccountCard({ row, now, onShowFailure })`; `Header` prop `compact: boolean`; `summarizeModels` result gains `label: string` (the top model's label).

- [ ] **Step 1: Failing test for `summarizeModels.label`**

In `src/lib/present.test.ts`, the two `summarizeModels` expectations become:

```ts
    expect(summarizeModels([{ label: "Fable", pct: 47, resets_at: null }])).toEqual({ pct: 47, label: "Fable", note: "Fable", title: "Fable 47%" });
    expect(summarizeModels([
      { label: "Opus", pct: 12, resets_at: null },
      { label: "Fable", pct: 47, resets_at: null },
    ])).toEqual({ pct: 47, label: "Fable", note: "Fable · +1", title: "Opus 12% · Fable 47%" });
```

Run `npm test -- src/lib/present.test.ts` — expected FAIL. Then in `src/lib/present.ts`: `export interface ModelSummary { pct: number; label: string; note: string; title: string }` and add `label: top.label,` to the returned object. Run again — PASS.

- [ ] **Step 2: StatusPill, Ring, AccountCard**

`src/components/StatusPill.tsx` (moved verbatim out of `AccountRow.tsx`, now exported):

```tsx
import type { JSX } from "react";
import type { Pill } from "../lib/pill";

export function StatusPill({ pill, onShowFailure }: { pill: Pill; onShowFailure: (id: number) => void }): JSX.Element {
  const cls = `pill pill-${pill.kind} pill-tone-${pill.tone}`;
  const id = pill.snapshotId;
  if (id !== undefined && pill.outcome !== "ok") {
    return (
      <button type="button" className={cls} title={pill.tooltip} onClick={() => onShowFailure(id)}>
        {pill.label}
      </button>
    );
  }
  return <span className={cls} title={pill.tooltip}>{pill.label}</span>;
}
```

In `src/components/AccountRow.tsx`: delete the local `StatusPill` function, add `import { StatusPill } from "./StatusPill";`, and change `import { type Pill, statusPill } from "../lib/pill";` to `import { statusPill } from "../lib/pill";`.

`src/components/Ring.tsx`:

```tsx
import type { JSX } from "react";
import { RING, ringDash } from "../lib/gauge";
import { metricColor } from "../lib/theme";

interface Props { pct: number | null; label: string; title?: string }

/**
 * A single-value meter bent into a circle: track in the grid colour, arc in
 * the same status colour the linear Meter uses, value in text ink (never
 * the series colour), label under. Arc starts at 12 o'clock.
 */
export function Ring({ pct, label, title }: Props): JSX.Element {
  const { circumference, offset } = ringDash(pct, RING.radius);
  const c = RING.size / 2;
  const text = pct === null ? "—" : `${Math.round(Math.max(0, Math.min(100, pct)))}%`;
  return (
    <div className="ring" title={title} role="img" aria-label={`${label} ${text}`}>
      <svg width={RING.size} height={RING.size} viewBox={`0 0 ${RING.size} ${RING.size}`} aria-hidden="true">
        <circle cx={c} cy={c} r={RING.radius} fill="none" stroke="var(--track-off)" strokeWidth={RING.stroke} />
        {pct !== null && pct > 0 && (
          <circle cx={c} cy={c} r={RING.radius} fill="none" stroke={metricColor(pct)} strokeWidth={RING.stroke}
            strokeLinecap="round" strokeDasharray={circumference} strokeDashoffset={offset}
            transform={`rotate(-90 ${c} ${c})`} />
        )}
        <text x={c} y={c} className="ring-value" textAnchor="middle" dominantBaseline="central">{text}</text>
      </svg>
      <span className="ring-label">{label}</span>
    </div>
  );
}
```

`src/components/AccountCard.tsx`:

```tsx
import type { JSX } from "react";
import { statusPill } from "../lib/pill";
import { accountDotColor, summarizeModels } from "../lib/present";
import type { AccountRow as AccountRowData } from "../lib/types";
import { Ring } from "./Ring";
import { StatusPill } from "./StatusPill";

interface Props { row: AccountRowData; now: number; onShowFailure: (id: number) => void }

/** Compact layout: name, status, and three ring gauges. No drag, no drawers. */
export function AccountCard({ row, now, onShowFailure }: Props): JSX.Element {
  const pill = statusPill(row, now);
  const models = summarizeModels(row.latest?.week_models ?? []);
  return (
    <div className="card" role="listitem">
      <div className="acct">
        <span className="acct-dot" style={{ background: accountDotColor(row, pill) }} />
        <span className={`acct-name${row.account.enabled ? "" : " acct-name-off"}`} title={row.account.config_dir}>{row.account.label}</span>
        {pill.tone !== "success" && <StatusPill pill={pill} onShowFailure={onShowFailure} />}
      </div>
      <div className="card-rings">
        <Ring pct={row.latest?.session?.pct ?? null} label="session" />
        <Ring pct={row.latest?.week_all?.pct ?? null} label="week" />
        {models === null
          ? <Ring pct={null} label="model" />
          : <Ring pct={models.pct} label={models.label} title={models.title} />}
      </div>
    </div>
  );
}
```

- [ ] **Step 3: Header and App**

`src/components/Header.tsx`: add `compact: boolean;` to `Props`, destructure it, and wrap the left block:

```tsx
        {!compact && (
          <div className="topbar-left">
            <h1 className="topbar-title">Usage Tracker</h1>
            <span className="topbar-count">{accountCountLabel(dashboard.accounts.length)}</span>
          </div>
        )}
```

`src/App.tsx`: pass `compact={layout === "cards"}` to `Header`; add `import { AccountCard } from "./components/AccountCard";`; replace the `AccountsTable` element with:

```tsx
        {layout === "cards" ? (
          <div className="cards" role="list" aria-label="Accounts">
            {dashboard.accounts.map((row) => (
              <AccountCard key={row.account.id} row={row} now={now} onShowFailure={(id) => setFailureId(id)} />
            ))}
          </div>
        ) : (
          <AccountsTable
            rows={dashboard.accounts}
            history={history}
            now={now}
            cycle={cycle}
            zoom={zoom}
            columnOrder={visible}
            onColumnMove={(from, to) => update({ columnOrder: moveVisible(prefs.columnOrder, effectiveHidden, from, to) })}
            onChanged={refetch}
            onError={showError}
            onShowFailure={(id) => setFailureId(id)}
          />
        )}
```

- [ ] **Step 4: CSS and window minimum**

Append to `src/styles.css` after the `/* ---- updated cell ---- */` block (do not touch `.app`; `SHELL_PADDING` is computed from its current 22 px sides):

```css
/* ---- compact cards ---- */
.cards { display: flex; flex-direction: column; gap: 10px; }
.card {
  display: flex; flex-direction: column; gap: 10px;
  padding: 12px 14px; border: 1px solid var(--line); border-radius: 12px; background: var(--panel);
}
.card-rings { display: flex; flex-wrap: wrap; gap: 10px 14px; }
.ring { display: flex; flex-direction: column; align-items: center; gap: 4px; width: 64px; }
.ring-value { font-family: var(--mono); font-size: 11px; fill: #dbe1e6; font-variant-numeric: tabular-nums; }
.ring-label { font-family: var(--mono); font-size: 10px; color: var(--muted); max-width: 64px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
```

`src-tauri/tauri.conf.json`: `"minWidth": 360, "minHeight": 240`.

- [ ] **Step 5: Run all four gates, then visual checks**

Gates as always. Visual (mock + Playwright): resize to 980×700 → full table; 720×600 → table without Updated and Per model, and Settings shows those two Column buttons disabled with the hint; 420×500 → cards (three rings per account, header without title, chip present, the failure pill on the third account opens the failure dialog); 360×240 → cards still fit without horizontal scroll (`document.documentElement.scrollWidth <= window.innerWidth`). Screenshots to the scratchpad.

- [ ] **Step 6: Commit**

```bash
git add src/components src/lib/present.ts src/lib/present.test.ts src/App.tsx src/styles.css src-tauri/tauri.conf.json
git commit -m "feat(ui): compact card layout with ring gauges below 640 px"
```

---

### Task 11: Always on top

**Files:**
- Modify: `src-tauri/capabilities/default.json`, `src/App.tsx`, `src/components/Settings.tsx`

**Interfaces:**
- Consumes: Task 5 (`Backend.setAlwaysOnTop`), Task 7 (`Prefs.alwaysOnTop`).
- Produces: Settings toggle "Keep window on top"; `App` applies the pref on mount and on change.

- [ ] **Step 1: Capability**

`src-tauri/capabilities/default.json` permissions gain `"core:window:allow-set-always-on-top"` after `"core:window:allow-unminimize"`.

- [ ] **Step 2: Apply the pref**

In `src/App.tsx`, add imports `import { backend } from "./lib/backend";` and `import { errorMessage } from "./lib/errors";`, and after the toast cleanup effect add:

```ts
  useEffect(() => {
    let cancelled = false;
    const apply = async (): Promise<void> => {
      try {
        await backend().setAlwaysOnTop(prefs.alwaysOnTop);
      } catch (e) {
        if (!cancelled) showError(errorMessage(e));
      }
    };
    void apply();
    return () => { cancelled = true; };
  }, [prefs.alwaysOnTop, showError]);
```

- [ ] **Step 3: Settings toggle**

In `src/components/Settings.tsx`, inside `<div className="toggles">`, after the "Debug logging" toggle:

```tsx
        <Toggle
          label="Keep window on top"
          hint="stay visible over other apps"
          checked={prefs.alwaysOnTop}
          onChange={(next) => onPrefs({ alwaysOnTop: next })}
        />
```

- [ ] **Step 4: Gates and a real-window check**

Run all four gates. First, against the mock in the browser: toggle "Keep window on top" on and off and confirm the console logs `mock: setAlwaysOnTop true` then `false`, the toggle state persists across a reload (`localStorage["usage-tracker.prefs.v1"]` contains `"alwaysOnTop":true` while on), and no toast appears. Then stop any running release instance of the app, run `npm run tauri dev`, and confirm what is observable from the driven window: it resizes down to 360×240 and switches to cards; toggling "Keep window on top" on and off produces **no toast** (a missing `core:window:allow-set-always-on-top` capability surfaces as an error toast through `onError`, so "no toast" is the capability check); the sparkline and the drawer load against the real backend. Whether the window actually floats over other apps is left for Josh to eyeball; say so in the task report. Close the dev app.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/capabilities/default.json src/App.tsx src/components/Settings.tsx
git commit -m "feat(ui): keep-window-on-top toggle persisted in prefs"
```

---

### Task 12: Docs, final verification, PR

**Files:**
- Modify: `docs/2026-09-15-claude-usage-tracker-design.md`, `README.md`

- [ ] **Step 1: Docs**

- In `docs/2026-09-15-claude-usage-tracker-design.md` §8, replace the `get_history` row with:

```
| `get_history` | `{account_id, since, bucket_ms, metric}` → `[{t, pct}]` buckets of max(metric) anchored at `since`; `metric` is `{kind:"week_all"}`, `{kind:"session"}` or `{kind:"model", label}`. Limits (2026-09-16): `bucket_ms ≥ 60 000`, `≤ 1 000` buckets, range `≤ 30 d` (+5 min slack), model label non-blank and `≤ 64` chars; violations are `out_of_range`. The sparkline asks for 7 d / 1 h week-all once per cycle; the drawer fetches on demand |
| `get_history_models` | `{account_id}` → `[label]` distinct model labels seen in `ok` snapshots within retention (2026-09-16) |
```

- In the same file, every remaining mention of `is_default` gets a one-line note (spec §6): find them with `grep -n is_default docs/2026-09-15-claude-usage-tracker-design.md` (at the time of writing: the `add_account` signature around line 193, the accounts DDL around line 509, and the `ORDER BY sort_order ASC, is_default DESC, …` around line 525) and append ` *(2026-09-16: `is_default` removed; V3 drops the column, `add_account` no longer takes it, ordering is `sort_order` then label.)*` to each line or the sentence introducing it.
- In the same file §7, after the accounts-table bullet, add: `- (2026-09-16) The history drawer offers range presets 1h/6h/12h/24h/7d/30d, a granularity override within the limits above, and a metric picker. Columns other than Account can be hidden in Settings. Below 820 px (local) the table hides Updated and Per model; below 640 px it becomes one card per account with ring gauges; minimum window 360×240. "Keep window on top" is a per-device pref. The default-account flag (`is_default`) was removed end to end.`
- In `README.md`, if it documents the window size, prefs or the `get_history` shape, update those lines to match; otherwise no change.

- [ ] **Step 2: Final gates and a clean-tree check**

Run all four gates once more. `git status` shows nothing unstaged except `.serena/`. `grep -rn "is_default\|HISTORY_DAYS\|onColumnOrder" src src-tauri/src` returns only `schema.rs` hits.

- [ ] **Step 3: Commit and open the PR**

```bash
git add docs README.md
git commit -m "docs: history query limits, compact view and column hiding"
git push -u origin history-and-compact
```

Load the `coderabbit-budget` skill before opening the PR; then `gh pr create --base master --title "History range/granularity, hideable columns, compact view" --body-file <scratchpad body>` where the body lists the three features, the default-flag removal, the corrected breakpoints (820 / 640, not the 660 / 500 first proposed), the visual screenshots, and ends with the attribution block from the session reminder. Merge per CLAUDE.md once CI and review are done: `gh pr merge <n> --squash --admin --delete-branch`.
