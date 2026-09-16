use rusqlite::{params, Connection, Row, ToSql};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::discovery::Candidate;
use crate::error::{AppError, AppResult};
use crate::store::Store;
use crate::usage::{Account, DisabledReason};

/// D17 (revised 2026-09-16): `sort_order` is authoritative; `is_default`
/// and label remain a tiebreak (relevant only while rows share a
/// `sort_order`, which normal use never produces once every row has gone
/// through an insert or `reorder_accounts`).
const ORDER_ACCOUNTS: &str =
    "ORDER BY sort_order ASC, is_default DESC, lower(label) ASC, label ASC";

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
        sort_order: row.get("sort_order")?,
    })
}

fn label_for(dir: &Path) -> String {
    dir.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.trim_start_matches('.').to_string())
        .unwrap_or_else(|| dir.to_string_lossy().to_string())
}

const SELECT_COLS: &str =
    "id, label, config_dir, enabled, disabled_reason, is_default, created_at, sort_order";

/// True for a UNIQUE or PRIMARY KEY constraint violation, which is the
/// signature of a raced insert against the `accounts.config_dir` UNIQUE
/// index (see `insert_account`). Any other SQLite error is left alone.
fn is_unique_violation(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code,
            },
            _,
        ) if *extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
            || *extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
    )
}

/// Maps an `INSERT` failure to `AppError::Duplicate` when it was caused by
/// the `config_dir` UNIQUE constraint, so a raced insert reports the same
/// error as the up-front existence check. Kept local to this module rather
/// than folded into `error.rs`'s generic `From<rusqlite::Error>`, which has
/// no way to distinguish a duplicate account from any other constraint
/// failure.
fn map_unique_violation(e: rusqlite::Error, canonical_str: &str) -> AppError {
    if is_unique_violation(&e) {
        AppError::Duplicate(format!("already tracked: {canonical_str}"))
    } else {
        AppError::from(e)
    }
}

/// Validates and canonicalises `config_dir`, and builds the `Account` row
/// (with a fresh id) without touching the database. Split out of
/// `add_account` so callers that need to insert several rows under one
/// `with_conn` closure (`seed_accounts_if_empty`) can build each row first
/// and let `insert_account` do the atomic check-and-insert.
fn build_account(
    config_dir: &Path,
    enabled: bool,
    disabled_reason: Option<DisabledReason>,
    is_default: bool,
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
        is_default,
        created_at: now,
        sort_order,
    };
    Ok((account, canonical_str))
}

/// `sort_order` for a newly inserted row: one past the current maximum, or 0
/// for the first account. Spec: new accounts from `add_account`,
/// `seed_accounts_if_empty` and `rescan_accounts` append at the end.
fn next_sort_order(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM accounts",
        [],
        |r| r.get(0),
    )?)
}

/// Existence check plus `INSERT`, run against a single already-locked
/// `Connection` so no other call on this `Store` can interleave between the
/// two (the `Mutex<Connection>` in `with_conn` is held for the whole
/// closure). The UNIQUE constraint on `config_dir` is the backstop: even if
/// this check somehow raced (e.g. a future refactor calls this per-row
/// inside a shared transaction with yielding), the mapped `INSERT` failure
/// still reports `duplicate`, never a bare `db` error.
fn insert_account(conn: &rusqlite::Connection, account: &Account, canonical_str: &str) -> AppResult<()> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM accounts WHERE config_dir = ?1",
        params![canonical_str],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Err(AppError::Duplicate(format!(
            "already tracked: {canonical_str}"
        )));
    }
    conn.execute(
        "INSERT INTO accounts(id, label, config_dir, enabled, disabled_reason, is_default, created_at, sort_order)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            account.id,
            account.label,
            canonical_str,
            i64::from(account.enabled),
            account.disabled_reason.map(|r| r.as_str()),
            i64::from(account.is_default),
            account.created_at,
            account.sort_order
        ],
    )
    .map_err(|e| map_unique_violation(e, canonical_str))?;
    Ok(())
}

impl Store {
    pub fn list_accounts(&self) -> AppResult<Vec<Account>> {
        self.with_conn(|c| {
            let sql = format!("SELECT {SELECT_COLS} FROM accounts {ORDER_ACCOUNTS}");
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
    /// already tracked (`duplicate`). The existence check and the `INSERT`
    /// run inside a single `with_conn` closure so the `Mutex<Connection>`
    /// serialises them against any other call on this `Store` — see
    /// `insert_account`.
    pub fn add_account(
        &self,
        config_dir: &Path,
        enabled: bool,
        disabled_reason: Option<DisabledReason>,
        is_default: bool,
        now: i64,
    ) -> AppResult<Account> {
        self.with_conn(|c| {
            let sort_order = next_sort_order(c)?;
            let (account, canonical_str) =
                build_account(config_dir, enabled, disabled_reason, is_default, sort_order, now)?;
            insert_account(c, &account, &canonical_str)?;
            Ok(account)
        })
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

    /// Spec 6.1: first start seeds every candidate **enabled**. The
    /// emptiness check and every insert run inside one `with_conn` closure
    /// (one `Mutex<Connection>` hold) so a concurrent `add_account` or
    /// `seed_accounts_if_empty` call can't interleave and double-seed.
    pub fn seed_accounts_if_empty(
        &self,
        candidates: &[Candidate],
        default_dir: &Path,
        now: i64,
    ) -> AppResult<usize> {
        let default_canonical =
            dunce::canonicalize(default_dir).unwrap_or_else(|_| default_dir.to_path_buf());

        self.with_conn(|c| {
            let existing: i64 =
                c.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
            if existing > 0 {
                return Ok(0);
            }

            // Resolve `is_default` for every candidate first, then sort to
            // D17 order (default first, then case-insensitive label) before
            // handing out `sort_order` 0..n-1, so a fresh seed's manual
            // order matches the order it always displayed in (spec D17,
            // revised 2026-09-16).
            let mut built: Vec<(Account, String)> = Vec::with_capacity(candidates.len());
            for cand in candidates {
                let (mut account, canonical_str) =
                    build_account(&cand.config_dir, true, None, false, 0, now)?;
                account.is_default = account.config_dir == default_canonical;
                built.push((account, canonical_str));
            }
            built.sort_by(|(a, _), (b, _)| {
                b.is_default
                    .cmp(&a.is_default)
                    .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
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

    /// Spec: manual account ordering. Assigns `sort_order` = position for
    /// each id in `ids`, in one transaction; any account NOT in `ids` keeps
    /// its relative order, appended after the listed ones. Returns the new
    /// full list. Unknown id -> `NotFound`; a duplicate id in `ids` ->
    /// `OutOfRange`.
    pub fn reorder_accounts(&self, ids: &[String]) -> AppResult<Vec<Account>> {
        let mut seen = HashSet::with_capacity(ids.len());
        for id in ids {
            if !seen.insert(id.as_str()) {
                return Err(AppError::OutOfRange("duplicate id in order".to_string()));
            }
        }

        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            for id in ids {
                let exists: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM accounts WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )?;
                if exists == 0 {
                    return Err(AppError::NotFound(format!("no such account: {id}")));
                }
            }

            for (pos, id) in ids.iter().enumerate() {
                tx.execute(
                    "UPDATE accounts SET sort_order = ?2 WHERE id = ?1",
                    params![id, pos as i64],
                )?;
            }

            // Untouched accounts keep their relative order, appended after
            // the listed ones. Their current `sort_order` still reflects
            // their pre-reorder relative order (only listed rows were just
            // rewritten), so ordering by it here is correct.
            if !ids.is_empty() {
                let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let sql = format!(
                    "SELECT id FROM accounts WHERE id NOT IN ({placeholders}) {ORDER_ACCOUNTS}"
                );
                let mut stmt = tx.prepare(&sql)?;
                let bound: Vec<&dyn ToSql> = ids.iter().map(|s| s as &dyn ToSql).collect();
                let others: Vec<String> = stmt
                    .query_map(bound.as_slice(), |r| r.get(0))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;

                for (next, id) in (ids.len() as i64..).zip(others) {
                    tx.execute(
                        "UPDATE accounts SET sort_order = ?2 WHERE id = ?1",
                        params![id, next],
                    )?;
                }
            }

            tx.commit()?;
            Ok(())
        })?;

        self.list_accounts()
    }
}

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
    fn unique_constraint_violations_map_to_duplicate_but_other_codes_dont() {
        fn synth(extended_code: i32) -> rusqlite::Error {
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error {
                    code: rusqlite::ErrorCode::ConstraintViolation,
                    extended_code,
                },
                None,
            )
        }

        assert!(matches!(
            map_unique_violation(synth(rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE), "x"),
            AppError::Duplicate(_)
        ));
        assert!(matches!(
            map_unique_violation(synth(rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY), "x"),
            AppError::Duplicate(_)
        ));
        assert!(!matches!(
            map_unique_violation(synth(rusqlite::ffi::SQLITE_CONSTRAINT_NOTNULL), "x"),
            AppError::Duplicate(_)
        ));
        assert!(!matches!(
            map_unique_violation(rusqlite::Error::QueryReturnedNoRows, "x"),
            AppError::Duplicate(_)
        ));
    }

    #[test]
    fn add_account_appends_in_call_order_regardless_of_label_or_default() {
        // D17 revised 2026-09-16: sort_order is authoritative once accounts
        // exist, so add_account (which only ever appends) no longer
        // resorts by label or is_default -- that would silently fight any
        // manual order the user has set via reorder_accounts.
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
        assert_eq!(labels, vec!["claudeZed", "claudealpha", "claudeMain"]);
    }

    #[test]
    fn seeding_orders_the_default_first_even_when_its_label_sorts_last() {
        // The D17 tiebreak (default first, then label) still governs a
        // fresh seed, which has no manual order yet to respect.
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let zzz = make_dir(tmp.path(), ".claudeZzzDefault");
        let aaa = make_dir(tmp.path(), ".claudeAaa");

        store
            .seed_accounts_if_empty(
                &[candidate(&aaa, "claudeAaa"), candidate(&zzz, "claudeZzzDefault")],
                &zzz,
                NOW,
            )
            .expect("seed");

        let accounts = store.list_accounts().expect("list");
        assert_eq!(accounts[0].label, "claudeZzzDefault");
        assert!(accounts[0].is_default);
        assert_eq!(accounts[0].sort_order, 0);
        assert_eq!(accounts[1].label, "claudeAaa");
        assert_eq!(accounts[1].sort_order, 1);
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

    fn seed_three(store: &Store, tmp: &std::path::Path) -> (String, String, String) {
        let a = make_dir(tmp, ".claudeA");
        let b = make_dir(tmp, ".claudeB");
        let c = make_dir(tmp, ".claudeC");
        let a = store.add_account(&a, true, None, false, NOW).expect("a").id;
        let b = store.add_account(&b, true, None, false, NOW).expect("b").id;
        let c = store.add_account(&c, true, None, false, NOW).expect("c").id;
        (a, b, c)
    }

    #[test]
    fn reorder_accounts_reorders_a_full_list() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let (a, b, c) = seed_three(&store, tmp.path());

        let result = store
            .reorder_accounts(&[c.clone(), a.clone(), b.clone()])
            .expect("reorder");
        let ids: Vec<String> = result.into_iter().map(|acc| acc.id).collect();
        assert_eq!(ids, vec![c.clone(), a.clone(), b.clone()]);

        // Persisted, not just returned.
        let relisted: Vec<String> = store
            .list_accounts()
            .expect("list")
            .into_iter()
            .map(|acc| acc.id)
            .collect();
        assert_eq!(relisted, vec![c, a, b]);
    }

    #[test]
    fn reorder_accounts_with_a_partial_list_appends_the_untouched_accounts_after() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let (a, b, c) = seed_three(&store, tmp.path());

        // Only move c to the front; a and b are untouched and must keep
        // their relative order (a before b), appended after c.
        let result = store
            .reorder_accounts(std::slice::from_ref(&c))
            .expect("reorder");
        let ids: Vec<String> = result.into_iter().map(|acc| acc.id).collect();
        assert_eq!(ids, vec![c, a, b]);
    }

    #[test]
    fn reorder_accounts_rejects_an_unknown_id_and_touches_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let (a, b, c) = seed_three(&store, tmp.path());

        let err = store
            .reorder_accounts(&[c.clone(), "nope".to_string(), a.clone()])
            .expect_err("must reject");
        assert_eq!(err.code(), "not_found");

        // Original order is untouched.
        let ids: Vec<String> = store
            .list_accounts()
            .expect("list")
            .into_iter()
            .map(|acc| acc.id)
            .collect();
        assert_eq!(ids, vec![a, b, c]);
    }

    #[test]
    fn reorder_accounts_rejects_a_duplicate_id_in_the_list() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let (a, b, _c) = seed_three(&store, tmp.path());

        let err = store
            .reorder_accounts(&[a.clone(), b.clone(), a])
            .expect_err("must reject");
        assert_eq!(err.code(), "out_of_range");
    }
}
