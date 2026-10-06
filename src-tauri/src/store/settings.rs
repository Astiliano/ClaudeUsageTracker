use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::store::Store;

pub const MIN_INTERVAL_SECS: u32 = 10;
pub const MAX_INTERVAL_SECS: u32 = 3600;
pub const DEFAULT_INTERVAL_SECS: u32 = 60;
pub const MIN_TIMEOUT_SECS: u32 = 5;
pub const MAX_TIMEOUT_SECS: u32 = 120;
pub const DEFAULT_TIMEOUT_SECS: u32 = 30;
/// Spec 4.6: below this much free commit, automatic refreshes hold.
/// 0 means never hold.
pub const DEFAULT_MIN_FREE_MEMORY_MB: u32 = 1536;
pub const MAX_MIN_FREE_MEMORY_MB: u32 = 65536;

pub const KEY_POLLING_HALTED: &str = "polling_halted";

/// The user-facing settings struct carried by `get_settings` / `set_settings`.
/// `launch_at_login` is on the wire but never in the table: the command layer
/// reads and writes the autostart plugin's live state (spec 6.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSettings {
    pub interval_secs: u32,
    pub timeout_secs: u32,
    pub claude_binary: String,
    pub close_to_tray: bool,
    pub launch_at_login: bool,
    pub log_level: String,
    pub min_free_memory_mb: u32,
}

/// D5 and spec 6.3/7 clamps. Rejection code is `out_of_range`.
pub fn validate_settings(s: &UserSettings) -> AppResult<()> {
    if !(MIN_INTERVAL_SECS..=MAX_INTERVAL_SECS).contains(&s.interval_secs) {
        return Err(AppError::OutOfRange(format!(
            "interval_secs must be {MIN_INTERVAL_SECS}..={MAX_INTERVAL_SECS}, got {}",
            s.interval_secs
        )));
    }
    if !(MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(&s.timeout_secs) {
        return Err(AppError::OutOfRange(format!(
            "timeout_secs must be {MIN_TIMEOUT_SECS}..={MAX_TIMEOUT_SECS}, got {}",
            s.timeout_secs
        )));
    }
    if s.log_level != "info" && s.log_level != "debug" {
        return Err(AppError::OutOfRange(format!(
            "log_level must be info or debug, got {}",
            s.log_level
        )));
    }
    if s.min_free_memory_mb > MAX_MIN_FREE_MEMORY_MB {
        return Err(AppError::OutOfRange(format!(
            "min_free_memory_mb must be 0..={MAX_MIN_FREE_MEMORY_MB}, got {}",
            s.min_free_memory_mb
        )));
    }
    Ok(())
}

/// D16 / spec 8: only `interval_secs`, `timeout_secs` and `claude_binary`
/// are polling-relevant. A change to one of them publishes on the settings
/// watch, which moves the deadline and resets backoff. `close_to_tray`,
/// `launch_at_login` and `log_level` are applied directly and must never
/// touch the scheduler.
pub fn polling_relevant_changed(previous: &UserSettings, next: &UserSettings) -> bool {
    previous.interval_secs != next.interval_secs
        || previous.timeout_secs != next.timeout_secs
        || previous.claude_binary != next.claude_binary
}

/// Spec 4.6: whether the driver must hear about a settings change. That is
/// every polling-relevant change plus a change of the memory floor, which the
/// driver reads at decide time but which must not reset backoff by itself
/// (the driver decides the reset, spec 4.3b).
pub fn driver_relevant_changed(previous: &UserSettings, next: &UserSettings) -> bool {
    polling_relevant_changed(previous, next)
        || previous.min_free_memory_mb != next.min_free_memory_mb
}

impl Store {
    pub fn get_raw(&self, key: &str) -> AppResult<Option<String>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT value FROM settings WHERE key = ?1")?;
            let mut rows = stmt.query([key])?;
            match rows.next()? {
                Some(row) => Ok(Some(row.get::<_, String>(0)?)),
                None => Ok(None),
            }
        })
    }

    pub fn set_raw(&self, key: &str, value: &str) -> AppResult<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO settings(key, value) VALUES(?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![key, value],
            )?;
            Ok(())
        })
    }

    fn get_u32(&self, key: &str, default: u32) -> AppResult<u32> {
        Ok(self
            .get_raw(key)?
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(default))
    }

    fn get_bool(&self, key: &str, default: bool) -> AppResult<bool> {
        Ok(self
            .get_raw(key)?
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(default))
    }

    /// Everything the table knows. `launch_at_login` is always `false` here.
    pub fn stored_settings(&self) -> AppResult<UserSettings> {
        Ok(UserSettings {
            interval_secs: self.get_u32("interval_secs", DEFAULT_INTERVAL_SECS)?,
            timeout_secs: self.get_u32("timeout_secs", DEFAULT_TIMEOUT_SECS)?,
            claude_binary: self.get_raw("claude_binary")?.unwrap_or_default(),
            close_to_tray: self.get_bool("close_to_tray", true)?,
            launch_at_login: false,
            log_level: self
                .get_raw("log_level")?
                .unwrap_or_else(|| "info".to_string()),
            min_free_memory_mb: self
                .get_u32("min_free_memory_mb", DEFAULT_MIN_FREE_MEMORY_MB)?,
        })
    }

    /// Validates first, so an out-of-range value never reaches the table.
    pub fn save_settings(&self, s: &UserSettings) -> AppResult<()> {
        validate_settings(s)?;
        self.set_raw("interval_secs", &s.interval_secs.to_string())?;
        self.set_raw("timeout_secs", &s.timeout_secs.to_string())?;
        self.set_raw("claude_binary", &s.claude_binary)?;
        self.set_raw("close_to_tray", if s.close_to_tray { "1" } else { "0" })?;
        self.set_raw("log_level", &s.log_level)?;
        self.set_raw("min_free_memory_mb", &s.min_free_memory_mb.to_string())?;
        Ok(())
    }

    /// `None`, or `guard_tripped:<epoch ms>`. Survives restarts by design.
    pub fn polling_halted(&self) -> AppResult<Option<String>> {
        self.get_raw(KEY_POLLING_HALTED)
    }

    pub fn set_polling_halted(&self, value: &str) -> AppResult<()> {
        self.set_raw(KEY_POLLING_HALTED, value)
    }

    /// Clears the flag and returns whatever it held.
    pub fn clear_polling_halted(&self) -> AppResult<Option<String>> {
        let previous = self.polling_halted()?;
        self.with_conn(|c| {
            c.execute(
                "DELETE FROM settings WHERE key = ?1",
                rusqlite::params![KEY_POLLING_HALTED],
            )?;
            Ok(())
        })?;
        Ok(previous)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn defaults() -> UserSettings {
        UserSettings {
            interval_secs: DEFAULT_INTERVAL_SECS,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            claude_binary: String::new(),
            close_to_tray: true,
            launch_at_login: false,
            log_level: "info".to_string(),
            min_free_memory_mb: DEFAULT_MIN_FREE_MEMORY_MB,
        }
    }

    #[test]
    fn a_fresh_store_returns_the_documented_defaults() {
        let store = Store::open_in_memory().expect("open");
        let s = store.stored_settings().expect("read");
        assert_eq!(s.interval_secs, 60);
        assert_eq!(s.timeout_secs, 30);
        assert_eq!(s.claude_binary, "");
        assert!(s.close_to_tray);
        assert!(!s.launch_at_login);
        assert_eq!(s.log_level, "info");
        assert_eq!(s.min_free_memory_mb, 1536);
    }

    #[test]
    fn settings_round_trip() {
        let store = Store::open_in_memory().expect("open");
        let mut s = defaults();
        s.interval_secs = 120;
        s.timeout_secs = 45;
        s.claude_binary = "C:/bin/claude.exe".into();
        s.close_to_tray = false;
        s.log_level = "debug".into();
        s.min_free_memory_mb = 2048;
        store.save_settings(&s).expect("save");

        let back = store.stored_settings().expect("read");
        assert_eq!(back.interval_secs, 120);
        assert_eq!(back.timeout_secs, 45);
        assert_eq!(back.claude_binary, "C:/bin/claude.exe");
        assert!(!back.close_to_tray);
        assert_eq!(back.log_level, "debug");
        assert_eq!(back.min_free_memory_mb, 2048);
    }

    #[test]
    fn boundary_values_are_accepted() {
        for v in [MIN_INTERVAL_SECS, MAX_INTERVAL_SECS] {
            let mut s = defaults();
            s.interval_secs = v;
            validate_settings(&s).unwrap_or_else(|e| panic!("{v} must be valid: {e}"));
        }
        for v in [MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS] {
            let mut s = defaults();
            s.timeout_secs = v;
            validate_settings(&s).unwrap_or_else(|e| panic!("{v} must be valid: {e}"));
        }
    }

    #[test]
    fn one_off_values_are_rejected_as_out_of_range() {
        for v in [MIN_INTERVAL_SECS - 1, MAX_INTERVAL_SECS + 1] {
            let mut s = defaults();
            s.interval_secs = v;
            let err = validate_settings(&s).expect_err("must reject");
            assert_eq!(err.code(), "out_of_range");
        }
        for v in [MIN_TIMEOUT_SECS - 1, MAX_TIMEOUT_SECS + 1] {
            let mut s = defaults();
            s.timeout_secs = v;
            let err = validate_settings(&s).expect_err("must reject");
            assert_eq!(err.code(), "out_of_range");
        }
    }

    #[test]
    fn an_unknown_log_level_is_out_of_range() {
        let mut s = defaults();
        s.log_level = "trace".into();
        let err = validate_settings(&s).expect_err("must reject");
        assert_eq!(err.code(), "out_of_range");
    }

    #[test]
    fn save_settings_rejects_out_of_range_input_before_writing() {
        let store = Store::open_in_memory().expect("open");
        let mut s = defaults();
        s.interval_secs = 9;
        let err = store.save_settings(&s).expect_err("must reject");
        assert_eq!(err.code(), "out_of_range");
        assert_eq!(store.stored_settings().expect("read").interval_secs, 60);
    }

    #[test]
    fn launch_at_login_is_never_persisted() {
        let store = Store::open_in_memory().expect("open");
        let mut s = defaults();
        s.launch_at_login = true;
        store.save_settings(&s).expect("save");
        assert!(!store.stored_settings().expect("read").launch_at_login);
        assert_eq!(
            store.get_raw("launch_at_login").expect("read raw"),
            None,
            "launch_at_login must not reach the settings table"
        );
    }

    #[test]
    fn polling_halted_starts_absent_and_round_trips() {
        let store = Store::open_in_memory().expect("open");
        assert_eq!(store.polling_halted().expect("read"), None);
        store
            .set_polling_halted("guard_tripped:1700000000000")
            .expect("set");
        assert_eq!(
            store.polling_halted().expect("read"),
            Some("guard_tripped:1700000000000".to_string())
        );
    }

    #[test]
    fn clear_polling_halted_returns_the_previous_value() {
        let store = Store::open_in_memory().expect("open");
        store
            .set_polling_halted("guard_tripped:1700000000000")
            .expect("set");
        let prev = store.clear_polling_halted().expect("clear");
        assert_eq!(prev, Some("guard_tripped:1700000000000".to_string()));
        assert_eq!(store.polling_halted().expect("read"), None);
        assert_eq!(store.clear_polling_halted().expect("clear again"), None);
    }

    #[test]
    fn polling_halted_survives_close_and_reopen() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("usage.sqlite");
        {
            let store = Store::open(&path).expect("open");
            store
                .set_polling_halted("guard_tripped:1700000000000")
                .expect("set");
        }
        let store = Store::open(&path).expect("reopen");
        assert_eq!(
            store.polling_halted().expect("read"),
            Some("guard_tripped:1700000000000".to_string())
        );
    }

    #[test]
    fn saving_user_settings_does_not_touch_polling_halted() {
        let store = Store::open_in_memory().expect("open");
        store
            .set_polling_halted("guard_tripped:1700000000000")
            .expect("set");
        store.save_settings(&defaults()).expect("save");
        assert_eq!(
            store.polling_halted().expect("read"),
            Some("guard_tripped:1700000000000".to_string())
        );
    }

    #[test]
    fn only_the_three_polling_keys_are_scheduler_relevant() {
        let base = defaults();

        let mut interval = base.clone();
        interval.interval_secs = 120;
        assert!(polling_relevant_changed(&base, &interval));

        let mut timeout = base.clone();
        timeout.timeout_secs = 45;
        assert!(polling_relevant_changed(&base, &timeout));

        let mut binary = base.clone();
        binary.claude_binary = "C:/bin/claude.exe".into();
        assert!(polling_relevant_changed(&base, &binary));
    }

    #[test]
    fn the_other_three_keys_never_touch_the_scheduler() {
        let base = defaults();

        let mut tray = base.clone();
        tray.close_to_tray = false;
        assert!(!polling_relevant_changed(&base, &tray));

        let mut autostart = base.clone();
        autostart.launch_at_login = true;
        assert!(!polling_relevant_changed(&base, &autostart));

        let mut level = base.clone();
        level.log_level = "debug".into();
        assert!(!polling_relevant_changed(&base, &level));

        // The memory floor is a fourth non-polling key. It reaches the driver
        // through `driver_relevant_changed`, never through this predicate.
        let mut floor_only = base.clone();
        floor_only.min_free_memory_mb = 512;
        assert!(!polling_relevant_changed(&base, &floor_only));
    }

    #[test]
    fn the_memory_floor_accepts_0_and_65536_and_rejects_65537() {
        for v in [0, MAX_MIN_FREE_MEMORY_MB] {
            let mut s = defaults();
            s.min_free_memory_mb = v;
            validate_settings(&s).unwrap_or_else(|e| panic!("{v} must be valid: {e}"));
        }
        assert_eq!(MAX_MIN_FREE_MEMORY_MB, 65536);
        let mut s = defaults();
        s.min_free_memory_mb = 65537;
        assert!(matches!(
            validate_settings(&s),
            Err(AppError::OutOfRange(_))
        ));
    }

    #[test]
    fn an_out_of_range_memory_floor_never_reaches_the_table() {
        let store = Store::open_in_memory().expect("open");
        let mut s = defaults();
        s.min_free_memory_mb = 65537;
        let err = store.save_settings(&s).expect_err("must reject");
        assert_eq!(err.code(), "out_of_range");
        assert_eq!(store.get_raw("min_free_memory_mb").expect("raw"), None);
        assert_eq!(store.stored_settings().expect("read").min_free_memory_mb, 1536);
    }

    #[test]
    fn the_memory_floor_reaches_the_driver_but_is_not_polling_relevant() {
        let base = defaults();
        let mut floor = base.clone();
        floor.min_free_memory_mb = 512;
        assert!(driver_relevant_changed(&base, &floor));
        assert!(!polling_relevant_changed(&base, &floor));

        let mut interval = base.clone();
        interval.interval_secs = 120;
        assert!(driver_relevant_changed(&base, &interval));

        assert!(!driver_relevant_changed(&defaults(), &defaults()));
    }

    /// Migration: a database written by a version that predates the key has
    /// no `min_free_memory_mb` row. It must load with every old value intact
    /// and the floor at the documented default, with no schema change.
    #[test]
    fn a_database_from_before_the_floor_loads_unchanged_with_the_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("usage.sqlite");
        {
            let store = Store::open(&path).expect("open");
            store.set_raw("interval_secs", "120").expect("set");
            store.set_raw("timeout_secs", "45").expect("set");
            store.set_raw("claude_binary", "C:/bin/claude.exe").expect("set");
            store.set_raw("close_to_tray", "0").expect("set");
            store.set_raw("log_level", "debug").expect("set");
            assert_eq!(store.get_raw("min_free_memory_mb").expect("raw"), None);
        }
        let store = Store::open(&path).expect("reopen");
        let s = store.stored_settings().expect("read");
        assert_eq!(s.interval_secs, 120);
        assert_eq!(s.timeout_secs, 45);
        assert_eq!(s.claude_binary, "C:/bin/claude.exe");
        assert!(!s.close_to_tray);
        assert_eq!(s.log_level, "debug");
        assert_eq!(s.min_free_memory_mb, DEFAULT_MIN_FREE_MEMORY_MB);
        assert_eq!(s.min_free_memory_mb, 1536);
        // Reading never writes the default back.
        assert_eq!(store.get_raw("min_free_memory_mb").expect("raw"), None);
    }

    #[test]
    fn saving_over_a_pre_floor_database_adds_the_floor_and_keeps_the_rest() {
        let store = Store::open_in_memory().expect("open");
        store.set_raw("interval_secs", "90").expect("set");
        let mut s = store.stored_settings().expect("read");
        assert_eq!(s.min_free_memory_mb, 1536);
        s.min_free_memory_mb = 0;
        store.save_settings(&s).expect("save");
        let back = store.stored_settings().expect("read");
        assert_eq!(back.min_free_memory_mb, 0, "0 (never hold) is a stored value, not a missing one");
        assert_eq!(back.interval_secs, 90);
    }

    #[test]
    fn an_unparseable_stored_floor_reads_the_default() {
        let store = Store::open_in_memory().expect("open");
        store.set_raw("min_free_memory_mb", "lots").expect("set");
        assert_eq!(store.stored_settings().expect("read").min_free_memory_mb, 1536);
    }

    #[test]
    fn an_identical_save_is_not_a_change() {
        assert!(!polling_relevant_changed(&defaults(), &defaults()));
    }
}
