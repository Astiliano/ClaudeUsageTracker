pub mod accounts;
pub mod schema;
pub mod settings;

pub use settings::{validate_settings, UserSettings};

use rusqlite::Connection;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::error::AppResult;

/// Spec 6.6: a single `Mutex<Connection>`. Every public method here is
/// synchronous; callers wrap them in `spawn_blocking` so the mutex is never
/// held across an `await`.
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> AppResult<Store> {
        if let Some(parent) = path.parent() {
            crate::paths::ensure_dir(parent)?;
        }
        let conn = Connection::open(path)?;
        schema::apply_pragmas(&conn)?;
        schema::migrate(&conn)?;
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    /// Tests only in practice, but not gated on `cfg(test)` so integration
    /// tests can use it too.
    pub fn open_in_memory() -> AppResult<Store> {
        let conn = Connection::open_in_memory()?;
        schema::apply_pragmas(&conn)?;
        schema::migrate(&conn)?;
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    /// A poisoned mutex means a previous caller panicked while holding it.
    /// The connection itself is still usable, so recover rather than panic.
    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<T> {
        let guard = self.lock();
        f(&guard)
    }

    pub fn with_conn_mut<T>(
        &self,
        f: impl FnOnce(&mut Connection) -> AppResult<T>,
    ) -> AppResult<T> {
        let mut guard = self.lock();
        f(&mut guard)
    }
}
