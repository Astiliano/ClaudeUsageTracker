use rusqlite::Connection;

use crate::error::AppResult;

pub const SCHEMA_VERSION: i64 = 3;

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

/// V2 (spec D17 revised 2026-09-16): manual account ordering. Backfilled in
/// one pass to the pre-existing D17 order (default first, then
/// case-insensitive label) so a fresh migration never reorders anyone's
/// accounts on upgrade.
const V2_ALTER: &str = "ALTER TABLE accounts ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;";

const V2_BACKFILL: &str = r#"
WITH ordered AS (
  SELECT id, ROW_NUMBER() OVER (
    ORDER BY is_default DESC, lower(label) ASC, label ASC
  ) - 1 AS rn
  FROM accounts
)
UPDATE accounts SET sort_order = (SELECT rn FROM ordered WHERE ordered.id = accounts.id);
"#;

/// V3 (2026-09-16): the default-account flag is gone. `sort_order` has been
/// authoritative since V2 and nothing read `is_default` any more.
const V3_DROP: &str = "ALTER TABLE accounts DROP COLUMN is_default;";

/// Migrate forward using `PRAGMA user_version`. Idempotent. Each step's DDL
/// (and any backfill) is wrapped with its `user_version` bump in one
/// transaction, so a crash or a failed step can never leave the database
/// declaring a version it hasn't fully reached.
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
            .with_conn_mut(migrate)
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
    fn migrating_an_empty_database_adds_the_sort_order_column() {
        let store = Store::open_in_memory().expect("open");
        let has_column: bool = store
            .with_conn(|c| {
                let mut stmt = c.prepare("PRAGMA table_info(accounts)")?;
                let names = stmt
                    .query_map([], |r| r.get::<_, String>(1))?
                    .collect::<Result<Vec<String>, rusqlite::Error>>()?;
                Ok(names.iter().any(|n| n == "sort_order"))
            })
            .expect("table_info");
        assert!(has_column, "sort_order column must exist after a fresh migration");
    }

    #[test]
    fn migrating_from_a_v1_database_backfills_sort_order_in_d17_order_and_is_idempotent() {
        let mut conn = rusqlite::Connection::open_in_memory().expect("open");
        apply_pragmas(&conn).expect("pragmas");
        conn.execute_batch(V1).expect("create v1 schema by hand");
        conn.execute_batch("PRAGMA user_version=1;").expect("set v1");

        // Two accounts, inserted in an order that is NOT D17 order, exactly
        // as a real V1 database (pre-sort_order) would contain them.
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
        .expect("insert alpha (default)");

        migrate(&mut conn).expect("migrate v1 -> v2");

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("read user_version");
        assert_eq!(version, SCHEMA_VERSION);

        let ordered: Vec<(String, i64)> = conn
            .prepare("SELECT id, sort_order FROM accounts ORDER BY sort_order ASC")
            .expect("prepare")
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("query")
            .collect::<Result<Vec<(String, i64)>, rusqlite::Error>>()
            .expect("collect");
        // 'alpha' is the default account, so D17 order puts it first.
        assert_eq!(
            ordered,
            vec![("alpha".to_string(), 0), ("bravo".to_string(), 1)]
        );

        // A second migrate is a no-op: same version, same sort_order values.
        migrate(&mut conn).expect("second migrate must succeed");
        let version_again: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("read user_version");
        assert_eq!(version_again, SCHEMA_VERSION);
        let ordered_again: Vec<(String, i64)> = conn
            .prepare("SELECT id, sort_order FROM accounts ORDER BY sort_order ASC")
            .expect("prepare")
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("query")
            .collect::<Result<Vec<(String, i64)>, rusqlite::Error>>()
            .expect("collect");
        assert_eq!(ordered_again, ordered);
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
}
