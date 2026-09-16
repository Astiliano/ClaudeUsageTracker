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
            .with_conn(migrate)
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
