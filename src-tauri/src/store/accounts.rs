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
