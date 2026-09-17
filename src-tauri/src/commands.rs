use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::discovery::{enumerate_profiles, find_claude_binary};
use crate::error::{AppError, AppResult};
use crate::logging::LogHandle;
use crate::scheduler::machine::{preview_manual, DriverStatus};
use crate::scheduler::triggers::Triggers;
use crate::store::settings::{polling_relevant_changed, validate_settings, UserSettings};
use crate::store::{HistoryPoint, Store};
use crate::usage::{Account, SnapshotDto};
#[cfg(test)]
use crate::usage::{DisabledReason, PollOutcome};

/// `(path, source)` of the binary found at the driver's last check. Kept
/// beside `DriverStatus` rather than inside it because spec 5.1 defines
/// `DriverStatus` as scheduler state only.
pub type BinarySlot = Arc<Mutex<Option<(String, &'static str)>>>;

pub fn lock_status(s: &Mutex<DriverStatus>) -> MutexGuard<'_, DriverStatus> {
    s.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn lock_binary<'a>(
    b: &'a Mutex<Option<(String, &'static str)>>,
) -> MutexGuard<'a, Option<(String, &'static str)>> {
    b.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Everything a command needs, with no Tauri types, so the whole surface is
/// unit-testable. The `#[tauri::command]` wrappers are thin. There is
/// deliberately no `Machine` handle here: scheduler state is read only from
/// the published `DriverStatus` snapshot.
pub struct Core {
    pub store: Arc<Store>,
    pub triggers: Arc<Triggers>,
    pub status: Arc<Mutex<DriverStatus>>,
    pub binary: BinarySlot,
    /// In-memory mirror of the `polling_halted` flag, armed the instant the
    /// guard trips and *before* the store write is attempted. The store write
    /// can fail — a full disk, a locked database — and the flag is the
    /// safety property, so nothing may re-arm polling on the strength of a
    /// `polling_halted` read that only returned `None` because the write
    /// never landed. Every halted check is `latch || stored`, and only
    /// `core_clear_halt` lowers it.
    pub halt_latched: AtomicBool,
    /// Cache of `UserSettings::close_to_tray`, seeded at setup and rewritten
    /// by `core_set_settings`. The window-close handler runs on the UI thread
    /// and must never take the store's connection mutex, which parks for up
    /// to `busy_timeout` under contention.
    pub close_to_tray: AtomicBool,
    pub settings_tx: watch::Sender<UserSettings>,
    pub log: Option<Arc<LogHandle>>,
    pub app_data_dir: PathBuf,
    pub log_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct BinaryInfo {
    pub path: Option<String>,
    pub source: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountRow {
    pub account: Account,
    pub latest: Option<SnapshotDto>,
    pub backoff_until: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dashboard {
    pub accounts: Vec<AccountRow>,
    pub gate: &'static str,
    pub busy: bool,
    pub halted: Option<String>,
    pub stalled_at: Option<i64>,
    pub binary: BinaryInfo,
    pub interval_secs: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawSnapshot {
    pub raw: Option<String>,
    pub error: Option<String>,
}

/// Spec 6.6: every `Store` method is synchronous and the connection mutex is
/// never held across an `await`, so every async caller hops to the blocking
/// pool first. `busy_timeout=5000` means a contended statement can park a
/// thread for five seconds, which must never be a runtime worker.
///
/// Public because the scheduler driver uses the same helper for its own store
/// access; it is the single place that hop is expressed.
pub async fn blocking<T, F>(f: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Internal(format!("blocking task failed: {e}")))?
}

/// Cheap; called on every `usage:updated` (debounced in the frontend).
/// Copies the published `DriverStatus` and never reads `Machine` (spec 5.1).
pub fn core_get_dashboard(core: &Core) -> AppResult<Dashboard> {
    let accounts = core.store.list_accounts()?;
    let latest = core.store.latest_per_account()?;
    let settings = core.store.stored_settings()?;
    let halted = halted_value(core)?;

    let status = lock_status(&core.status).clone();
    let binary = {
        let found = lock_binary(&core.binary);
        BinaryInfo {
            path: found.as_ref().map(|(p, _)| p.clone()),
            source: found.as_ref().map(|(_, s)| *s),
        }
    };

    let rows = accounts
        .into_iter()
        .map(|account| AccountRow {
            latest: latest.get(&account.id).cloned(),
            backoff_until: status.backoff_until.get(&account.id).copied(),
            account,
        })
        .collect();

    Ok(Dashboard {
        accounts: rows,
        gate: status.gate.as_str(),
        busy: status.busy,
        halted,
        stalled_at: status.stalled_at,
        binary,
        interval_secs: settings.interval_secs,
    })
}

/// Longest history the UI can ask for; matches store retention (D10, 30 days).
pub const MAX_HISTORY_DAYS: u32 = 30;
const DEFAULT_HISTORY_DAYS: u32 = 7;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

pub fn core_get_history(core: &Core, account_id: &str, now: i64, days: u32) -> AppResult<Vec<HistoryPoint>> {
    let days = i64::from(days.clamp(1, MAX_HISTORY_DAYS));
    core.store.history(account_id, now - days * DAY_MS)
}

/// Fills the binary slot from the same resolver the driver uses.
///
/// Called once at setup, before the driver starts. Without it the slot is
/// empty until the driver's first `refresh_binary`, and a Refresh click in
/// that window is answered `skipped:no_binary` on a perfectly healthy
/// install. The driver keeps overwriting it from then on.
pub fn seed_binary_slot(slot: &BinarySlot, override_path: &str) {
    *lock_binary(slot) = find_claude_binary(Some(override_path))
        .map(|f| (f.path.to_string_lossy().to_string(), f.source.as_str()));
}

/// The halt as the UI should see it: the persisted value when the write
/// landed, and the synthetic `guard_tripped:unpersisted` when only the
/// in-memory latch is up (the guard tripped but the store write failed).
fn halted_value(core: &Core) -> AppResult<Option<String>> {
    match core.store.polling_halted()? {
        Some(v) => Ok(Some(v)),
        None if core.halt_latched.load(Ordering::SeqCst) => {
            Ok(Some("guard_tripped:unpersisted".to_string()))
        }
        None => Ok(None),
    }
}

/// `"started"` or `"skipped:<reason>"`. The preview reads the published
/// snapshot, and it is exact because a Manual trigger bypasses both the gate
/// and backoff.
pub fn core_poll_now(core: &Core) -> AppResult<String> {
    let halted =
        core.halt_latched.load(Ordering::SeqCst) || core.store.polling_halted()?.is_some();
    let binary_present = lock_binary(&core.binary).is_some();
    let enabled = core.store.enabled_account_ids()?;
    let status = lock_status(&core.status).clone();

    match preview_manual(&status, binary_present, halted, &enabled) {
        Some(reason) => {
            info!(reason = reason.as_str(), "manual poll skipped");
            Ok(format!("skipped:{}", reason.as_str()))
        }
        None => {
            core.triggers.manual();
            Ok("started".to_string())
        }
    }
}

pub fn core_add_account(core: &Core, config_dir: &Path, now: i64) -> AppResult<Account> {
    let account = core.store.add_account(config_dir, true, None, false, now)?;
    info!(account_id = %account.id, label = %account.label, "account added");
    core.triggers.account_changed(vec![account.id.clone()]);
    Ok(account)
}

/// `enabled: true` clears `disabled_reason` and triggers `AccountChanged`;
/// `enabled: false` sets the reason to `user` and polls nothing.
pub fn core_update_account(
    core: &Core,
    id: &str,
    label: Option<&str>,
    enabled: Option<bool>,
) -> AppResult<Account> {
    let account = core.store.update_account(id, label, enabled)?;
    // The backoff reset for this account happens inside `decide` when the
    // AccountChanged trigger is consumed; commands never touch the machine.
    if enabled == Some(true) {
        core.triggers.account_changed(vec![account.id.clone()]);
    }
    info!(account_id = %account.id, label = %account.label, enabled = account.enabled, "account updated");
    Ok(account)
}

pub fn core_remove_account(core: &Core, id: &str) -> AppResult<()> {
    core.store.remove_account(id)?;
    info!(account_id = id, "account removed");
    Ok(())
}

/// Manual account ordering (D17, revised 2026-09-16). Returns the new full
/// list so the caller can replace its local state with the persisted order.
pub fn core_reorder_accounts(core: &Core, ids: Vec<String>) -> AppResult<Vec<Account>> {
    let accounts = core.store.reorder_accounts(&ids)?;
    info!(count = accounts.len(), "accounts reordered");
    Ok(accounts)
}

/// Newly added accounts come back disabled (spec 6.1).
pub fn core_rescan_profiles(core: &Core, home: &Path, now: i64) -> AppResult<Vec<Account>> {
    let candidates = enumerate_profiles(home);
    let added = core.store.rescan_accounts(&candidates, now)?;
    info!(
        scanned = candidates.len(),
        added = added.len(),
        "profile rescan"
    );
    Ok(added)
}

/// `launch_at_login` is supplied by the caller from the autostart plugin.
pub fn core_get_settings(core: &Core, launch_at_login: bool) -> AppResult<UserSettings> {
    let mut s = core.store.stored_settings()?;
    s.launch_at_login = launch_at_login;
    Ok(s)
}

/// Validates, saves, and then applies the change in two distinct ways
/// (spec §8, D16):
///
/// * `interval_secs`, `timeout_secs` and `claude_binary` are polling-relevant,
///   so a change to any of them publishes on the settings watch. The driver's
///   watch arm is what moves the deadline and resets backoff — this function
///   does neither itself, because it has no machine handle.
/// * `close_to_tray`, `launch_at_login` and `log_level` are applied directly
///   and must never touch the scheduler. `log_level` goes through the reload
///   handle here; `launch_at_login` is written to the autostart plugin by the
///   Tauri wrapper; `close_to_tray` is simply read from the store when a
///   window close arrives.
///
/// Never touches `polling_halted`.
pub fn core_set_settings(core: &Core, next: &UserSettings) -> AppResult<()> {
    validate_settings(next)?;
    let previous = core.store.stored_settings()?;
    core.store.save_settings(next)?;

    if previous.log_level != next.log_level {
        if let Some(log) = core.log.as_ref() {
            log.set_level(&next.log_level)?;
        }
    }

    core.close_to_tray.store(next.close_to_tray, Ordering::SeqCst);

    let scheduler_affected = polling_relevant_changed(&previous, next);
    if scheduler_affected && core.settings_tx.send(next.clone()).is_err() {
        warn!("settings watch has no receiver; the driver may not be running");
    }

    info!(
        interval_secs = next.interval_secs,
        timeout_secs = next.timeout_secs,
        log_level = %next.log_level,
        scheduler_affected,
        "settings updated"
    );
    Ok(())
}

/// Which autostart call, if any, reconciles the plugin with the wanted
/// state. `None` for "already there" and for "state unknown and we only
/// want it off" (a blind disable fails on a missing registry value).
pub fn autostart_change(currently_enabled: Option<bool>, wanted: bool) -> Option<bool> {
    match (currently_enabled, wanted) {
        (Some(current), wanted) if current == wanted => None,
        (None, false) => None,
        (_, wanted) => Some(wanted),
    }
}

/// Clears the flag and logs the previous value at WARN. Does **not** poll:
/// every quota-spending action stays a separate, explicit act.
pub fn core_clear_halt(core: &Core) -> AppResult<()> {
    let previous = core.store.clear_polling_halted()?;
    // Lowered only once the store write has succeeded, so a failed clear
    // leaves the guard closed rather than half-open.
    let latched = core.halt_latched.swap(false, Ordering::SeqCst);
    warn!(previous = ?previous, latched, "polling halt cleared by the user");
    Ok(())
}

pub fn core_open_login(core: &Core, id: &str) -> AppResult<()> {
    let account = core
        .store
        .account_by_id(id)?
        .ok_or_else(|| AppError::NotFound(format!("no such account: {id}")))?;
    let override_path = core.store.stored_settings()?.claude_binary;
    let found = find_claude_binary(Some(&override_path)).ok_or_else(|| {
        AppError::NotFound("claude binary not found; set it in Settings".to_string())
    })?;
    crate::login::open_terminal_for_login(&core.app_data_dir, &found.path, &account.config_dir)
}

pub fn core_get_snapshot_raw(core: &Core, snapshot_id: i64) -> AppResult<RawSnapshot> {
    let (raw, error) = core.store.snapshot_raw(snapshot_id)?;
    Ok(RawSnapshot { raw, error })
}

use tauri::State;
use tauri_plugin_autostart::ManagerExt;

pub type SharedCore = Arc<Core>;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[tauri::command]
pub async fn get_dashboard(core: State<'_, SharedCore>) -> AppResult<Dashboard> {
    let core = Arc::clone(&core);
    blocking(move || core_get_dashboard(&core)).await
}

#[tauri::command]
pub async fn get_history(
    core: State<'_, SharedCore>,
    account_id: String,
    days: Option<u32>,
) -> AppResult<Vec<HistoryPoint>> {
    let core = Arc::clone(&core);
    let days = days.unwrap_or(DEFAULT_HISTORY_DAYS);
    blocking(move || core_get_history(&core, &account_id, now_ms(), days)).await
}

#[tauri::command]
pub async fn poll_now(core: State<'_, SharedCore>) -> AppResult<String> {
    let core = Arc::clone(&core);
    blocking(move || core_poll_now(&core)).await
}

#[tauri::command]
pub async fn add_account(
    app: tauri::AppHandle,
    core: State<'_, SharedCore>,
    config_dir: String,
) -> AppResult<Account> {
    let core = Arc::clone(&core);
    let tray_core = Arc::clone(&core);
    let account =
        blocking(move || core_add_account(&core, Path::new(&config_dir), now_ms())).await?;
    crate::tray::apply_tray(&app, &tray_core).await;
    Ok(account)
}

#[tauri::command]
pub async fn update_account(
    app: tauri::AppHandle,
    core: State<'_, SharedCore>,
    id: String,
    label: Option<String>,
    enabled: Option<bool>,
) -> AppResult<Account> {
    let core = Arc::clone(&core);
    let tray_core = Arc::clone(&core);
    let account =
        blocking(move || core_update_account(&core, &id, label.as_deref(), enabled)).await?;
    crate::tray::apply_tray(&app, &tray_core).await;
    Ok(account)
}

#[tauri::command]
pub async fn remove_account(
    app: tauri::AppHandle,
    core: State<'_, SharedCore>,
    id: String,
) -> AppResult<()> {
    let core = Arc::clone(&core);
    let tray_core = Arc::clone(&core);
    blocking(move || core_remove_account(&core, &id)).await?;
    crate::tray::apply_tray(&app, &tray_core).await;
    Ok(())
}

#[tauri::command]
pub async fn reorder_accounts(
    app: tauri::AppHandle,
    core: State<'_, SharedCore>,
    ids: Vec<String>,
) -> AppResult<Vec<Account>> {
    let core = Arc::clone(&core);
    let tray_core = Arc::clone(&core);
    let accounts = blocking(move || core_reorder_accounts(&core, ids)).await?;
    crate::tray::apply_tray(&app, &tray_core).await;
    Ok(accounts)
}

#[tauri::command]
pub async fn rescan_profiles(
    app: tauri::AppHandle,
    core: State<'_, SharedCore>,
) -> AppResult<Vec<Account>> {
    let core = Arc::clone(&core);
    let tray_core = Arc::clone(&core);
    let added = blocking(move || {
        let home = crate::paths::home_dir()?;
        core_rescan_profiles(&core, &home, now_ms())
    })
    .await?;
    crate::tray::apply_tray(&app, &tray_core).await;
    Ok(added)
}

#[tauri::command]
pub async fn get_settings(
    app: tauri::AppHandle,
    core: State<'_, SharedCore>,
) -> AppResult<UserSettings> {
    let core = Arc::clone(&core);
    // `is_enabled` reads the registry (Windows) or a launch-agent plist, so
    // it belongs on the blocking pool with the store read, not on a runtime
    // worker.
    blocking(move || {
        let launch_at_login = app.autolaunch().is_enabled().unwrap_or(false);
        core_get_settings(&core, launch_at_login)
    })
    .await
}

#[tauri::command]
pub async fn set_settings(
    app: tauri::AppHandle,
    core: State<'_, SharedCore>,
    settings: UserSettings,
) -> AppResult<()> {
    validate_settings(&settings)?;

    let want_autostart = settings.launch_at_login;
    let autostart_app = app.clone();
    blocking(move || {
        let current = match autostart_app.autolaunch().is_enabled() {
            Ok(v) => Some(v),
            Err(e) => {
                warn!(error = %e, "could not read launch-at-login state");
                None
            }
        };
        match autostart_change(current, want_autostart) {
            None => Ok(()),
            Some(true) => autostart_app
                .autolaunch()
                .enable()
                .map(|()| info!(want_autostart, "launch-at-login enabled")),
            Some(false) => autostart_app
                .autolaunch()
                .disable()
                .map(|()| info!(want_autostart, "launch-at-login disabled")),
        }
        .inspect_err(|e| warn!(error = %e, want_autostart, "could not update launch-at-login"))
        .map_err(|e| AppError::Internal(format!("could not update launch at login: {e}")))
    })
    .await?;

    let core_ref = Arc::clone(&core);
    let tray_core = Arc::clone(&core);
    let to_save = settings.clone();
    blocking(move || core_set_settings(&core_ref, &to_save)).await?;
    crate::tray::apply_tray(&app, &tray_core).await;
    Ok(())
}

#[tauri::command]
pub async fn clear_halt(app: tauri::AppHandle, core: State<'_, SharedCore>) -> AppResult<()> {
    let core = Arc::clone(&core);
    let tray_core = Arc::clone(&core);
    blocking(move || core_clear_halt(&core)).await?;
    // Clearing the guard must remove the Halted badge immediately, not on
    // the next poll cycle.
    crate::tray::apply_tray(&app, &tray_core).await;
    Ok(())
}

#[tauri::command]
pub async fn open_login(core: State<'_, SharedCore>, id: String) -> AppResult<()> {
    let core = Arc::clone(&core);
    blocking(move || core_open_login(&core, &id)).await
}

#[tauri::command]
pub async fn open_log_dir(app: tauri::AppHandle, core: State<'_, SharedCore>) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = core.log_dir.clone();
    crate::paths::ensure_dir(&dir)?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| AppError::Internal(format!("could not open the log folder: {e}")))
}

#[tauri::command]
pub async fn get_snapshot_raw(
    core: State<'_, SharedCore>,
    snapshot_id: i64,
) -> AppResult<RawSnapshot> {
    let core = Arc::clone(&core);
    blocking(move || core_get_snapshot_raw(&core, snapshot_id)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::machine::Gate;
    use crate::store::settings::UserSettings;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn defaults() -> UserSettings {
        UserSettings {
            interval_secs: 60,
            timeout_secs: 30,
            claude_binary: String::new(),
            close_to_tray: true,
            launch_at_login: false,
            log_level: "info".to_string(),
        }
    }

    fn core() -> (tempfile::TempDir, Arc<Core>) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(Store::open_in_memory().expect("open"));
        store.save_settings(&defaults()).expect("seed settings");
        let (settings_tx, _rx) = tokio::sync::watch::channel(defaults());
        let core = Arc::new(Core {
            store,
            triggers: Arc::new(Triggers::new()),
            status: Arc::new(std::sync::Mutex::new(DriverStatus::default())),
            binary: Arc::new(std::sync::Mutex::new(None)),
            halt_latched: AtomicBool::new(false),
            close_to_tray: AtomicBool::new(true),
            settings_tx,
            log: None,
            app_data_dir: tmp.path().to_path_buf(),
            log_dir: tmp.path().join("logs"),
        });
        (tmp, core)
    }

    /// Pretend the driver found a binary at its last check.
    fn with_binary(core: &Core) {
        *lock_binary(&core.binary) = Some(("/bin/claude".to_string(), "path"));
    }

    fn make_dir(root: &std::path::Path, name: &str) -> std::path::PathBuf {
        let d = root.join(name);
        std::fs::create_dir_all(&d).expect("mkdir");
        std::fs::write(d.join("settings.json"), "{}").expect("marker");
        d
    }

    #[test]
    fn add_account_rejects_a_missing_directory() {
        let (tmp, core) = core();
        let err = core_add_account(&core, &tmp.path().join("nope"), 1)
            .expect_err("must reject");
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn add_account_rejects_a_duplicate() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        core_add_account(&core, &d, 1).expect("first");
        let err = core_add_account(&core, &d, 2).expect_err("must reject");
        assert_eq!(err.code(), "duplicate");
    }

    #[test]
    fn adding_an_account_queues_an_account_changed_trigger() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &d, 1).expect("add");
        assert_eq!(core.triggers.take_changed(), vec![a.id]);
    }

    #[test]
    fn enabling_an_account_clears_the_reason_and_queues_a_trigger() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &d, 1).expect("add");
        let _ = core.triggers.take_changed();

        let off = core_update_account(&core, &a.id, None, Some(false)).expect("off");
        assert_eq!(off.disabled_reason, Some(DisabledReason::User));
        assert!(
            core.triggers.take_changed().is_empty(),
            "disabling must not queue a poll"
        );

        let on = core_update_account(&core, &a.id, None, Some(true)).expect("on");
        assert_eq!(on.disabled_reason, None);
        assert_eq!(core.triggers.take_changed(), vec![a.id]);
    }

    #[test]
    fn set_settings_accepts_the_boundary_values() {
        let (_tmp, core) = core();
        for (interval, timeout) in [(10u32, 5u32), (3600, 120)] {
            let mut s = defaults();
            s.interval_secs = interval;
            s.timeout_secs = timeout;
            core_set_settings(&core, &s).unwrap_or_else(|e| panic!("{interval}/{timeout}: {e}"));
        }
        assert_eq!(core.store.stored_settings().expect("read").interval_secs, 3600);
    }

    #[test]
    fn set_settings_rejects_one_off_values_and_keeps_the_previous() {
        let (_tmp, core) = core();
        for (interval, timeout) in [(9u32, 30u32), (3601, 30), (60, 4), (60, 121)] {
            let mut s = defaults();
            s.interval_secs = interval;
            s.timeout_secs = timeout;
            let err = core_set_settings(&core, &s).expect_err("must reject");
            assert_eq!(err.code(), "out_of_range");
        }
        assert_eq!(core.store.stored_settings().expect("read").interval_secs, 60);
    }

    #[test]
    fn a_polling_relevant_change_publishes_on_the_watch() {
        for mutate in [
            (|s: &mut UserSettings| s.interval_secs = 120) as fn(&mut UserSettings),
            |s: &mut UserSettings| s.timeout_secs = 45,
            |s: &mut UserSettings| s.claude_binary = "C:/bin/claude.exe".into(),
        ] {
            let (_tmp, core) = core();
            let mut rx = core.settings_tx.subscribe();
            let mut next = defaults();
            mutate(&mut next);

            core_set_settings(&core, &next).expect("set");

            assert!(
                rx.has_changed().unwrap_or(false),
                "a polling-relevant change must fire the watch"
            );
            assert_eq!(&*rx.borrow_and_update(), &next);
        }
    }

    #[test]
    fn the_other_three_keys_are_saved_without_touching_the_watch() {
        for mutate in [
            (|s: &mut UserSettings| s.close_to_tray = false) as fn(&mut UserSettings),
            |s: &mut UserSettings| s.launch_at_login = true,
            |s: &mut UserSettings| s.log_level = "debug".into(),
        ] {
            let (_tmp, core) = core();
            let rx = core.settings_tx.subscribe();
            let mut next = defaults();
            mutate(&mut next);

            core_set_settings(&core, &next).expect("set");

            assert!(
                !rx.has_changed().unwrap_or(false),
                "this key must not reach the scheduler"
            );
        }

        // ...and the value really was persisted.
        let (_tmp, core) = core();
        let mut next = defaults();
        next.log_level = "debug".into();
        next.close_to_tray = false;
        core_set_settings(&core, &next).expect("set");
        let stored = core.store.stored_settings().expect("read");
        assert_eq!(stored.log_level, "debug");
        assert!(!stored.close_to_tray);
    }

    #[test]
    fn saving_identical_settings_does_not_disturb_the_scheduler() {
        let (_tmp, core) = core();
        let rx = core.settings_tx.subscribe();
        core_set_settings(&core, &defaults()).expect("set");
        assert!(!rx.has_changed().unwrap_or(false));
    }

    #[test]
    fn set_settings_never_touches_the_halt_flag() {
        let (_tmp, core) = core();
        core.store
            .set_polling_halted("guard_tripped:1")
            .expect("halt");
        core_set_settings(&core, &defaults()).expect("set");
        assert_eq!(
            core.store.polling_halted().expect("read"),
            Some("guard_tripped:1".to_string())
        );
    }

    #[test]
    fn clear_halt_clears_the_flag_and_does_not_poll() {
        let (_tmp, core) = core();
        core.store
            .set_polling_halted("guard_tripped:1700000000000")
            .expect("halt");

        core_clear_halt(&core).expect("clear");

        assert_eq!(core.store.polling_halted().expect("read"), None);
        assert!(
            core.triggers.take_changed().is_empty(),
            "clear_halt must not queue an account-changed poll"
        );
        // Every quota-spending action stays a separate, explicit act: the
        // manual channel must have nothing pending.
        let pending = futures_lite_poll_once(&core.triggers);
        assert!(!pending, "clear_halt must not queue a manual poll");
    }

    /// True if a manual notification is already pending.
    fn futures_lite_poll_once(t: &Triggers) -> bool {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime");
        rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(50), t.notified_manual())
                .await
                .is_ok()
        })
    }

    #[test]
    fn poll_now_reports_the_skip_reason_when_halted() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        core_add_account(&core, &d, 1).expect("add");
        core.store.set_polling_halted("guard_tripped:1").expect("halt");
        with_binary(&core);
        assert_eq!(core_poll_now(&core).expect("poll"), "skipped:halted");
    }

    #[test]
    fn poll_now_reports_no_binary_and_no_enabled_accounts() {
        let (tmp, core) = core();
        assert_eq!(core_poll_now(&core).expect("poll"), "skipped:no_binary");

        with_binary(&core);
        assert_eq!(
            core_poll_now(&core).expect("poll"),
            "skipped:no_enabled_accounts"
        );

        let d = make_dir(tmp.path(), ".claude3");
        core_add_account(&core, &d, 1).expect("add");
        assert_eq!(core_poll_now(&core).expect("poll"), "started");
    }

    #[test]
    fn an_unpersisted_halt_latch_halts_poll_now_and_shows_in_the_dashboard() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        core_add_account(&core, &d, 1).expect("add");
        with_binary(&core);
        assert_eq!(core_poll_now(&core).expect("poll"), "started");

        // The guard tripped but the store write failed, so only the
        // in-memory latch is set (F1).
        core.halt_latched.store(true, Ordering::SeqCst);

        assert_eq!(core_poll_now(&core).expect("poll"), "skipped:halted");
        assert_eq!(
            core_get_dashboard(&core)
                .expect("dashboard")
                .halted
                .as_deref(),
            Some("guard_tripped:unpersisted")
        );

        core_clear_halt(&core).expect("clear");
        assert!(!core.halt_latched.load(Ordering::SeqCst));
        assert_eq!(core_get_dashboard(&core).expect("dashboard").halted, None);
        assert_eq!(core_poll_now(&core).expect("poll"), "started");
    }

    #[test]
    fn a_persisted_halt_value_is_reported_in_preference_to_the_latch() {
        let (_tmp, core) = core();
        core.store
            .set_polling_halted("guard_tripped:1700000000000")
            .expect("halt");
        core.halt_latched.store(true, Ordering::SeqCst);
        assert_eq!(
            core_get_dashboard(&core)
                .expect("dashboard")
                .halted
                .as_deref(),
            Some("guard_tripped:1700000000000")
        );
    }

    #[test]
    fn set_settings_keeps_the_close_to_tray_cache_in_step() {
        let (_tmp, core) = core();
        assert!(core.close_to_tray.load(Ordering::SeqCst));

        let mut next = defaults();
        next.close_to_tray = false;
        core_set_settings(&core, &next).expect("set");
        assert!(!core.close_to_tray.load(Ordering::SeqCst));

        next.close_to_tray = true;
        core_set_settings(&core, &next).expect("set");
        assert!(core.close_to_tray.load(Ordering::SeqCst));
    }


    #[test]
    fn autostart_change_is_a_noop_when_already_off() {
        assert_eq!(autostart_change(Some(false), false), None);
    }

    #[test]
    fn autostart_change_is_a_noop_when_already_on() {
        assert_eq!(autostart_change(Some(true), true), None);
    }

    #[test]
    fn autostart_change_enables_when_currently_off() {
        assert_eq!(autostart_change(Some(false), true), Some(true));
    }

    #[test]
    fn autostart_change_disables_when_currently_on() {
        assert_eq!(autostart_change(Some(true), false), Some(false));
    }

    #[test]
    fn autostart_change_enables_when_state_is_unknown() {
        assert_eq!(autostart_change(None, true), Some(true));
    }

    #[test]
    fn autostart_change_skips_a_blind_disable_when_state_is_unknown() {
        assert_eq!(autostart_change(None, false), None);
    }

    #[test]
    fn seeding_the_binary_slot_removes_the_spurious_no_binary_answer() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        core_add_account(&core, &d, 1).expect("add");
        // An empty slot is what the setup-to-first-decision window looked
        // like before M4.
        assert_eq!(core_poll_now(&core).expect("poll"), "skipped:no_binary");

        let exe = tmp
            .path()
            .join(if cfg!(windows) { "claude.exe" } else { "claude" });
        std::fs::write(&exe, b"#!/bin/sh
exit 0
").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }

        seed_binary_slot(&core.binary, &exe.to_string_lossy());

        assert_eq!(
            lock_binary(&core.binary).as_ref().map(|(_, s)| *s),
            Some("override"),
            "setup must publish the resolved binary"
        );
        assert_eq!(core_poll_now(&core).expect("poll"), "started");
    }

    #[test]
    fn poll_now_reports_busy_from_the_published_snapshot() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        core_add_account(&core, &d, 1).expect("add");
        with_binary(&core);
        lock_status(&core.status).busy = true;
        assert_eq!(core_poll_now(&core).expect("poll"), "skipped:busy");
    }

    #[test]
    fn the_dashboard_copies_the_published_snapshot() {
        let (tmp, core) = core();
        let dir = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &dir, 1).expect("add");
        core.store
            .insert_snapshot(&a.id, 1000, &PollOutcome::Timeout(30), Some("raw"), 5)
            .expect("snapshot");
        *lock_binary(&core.binary) = Some(("/bin/claude".to_string(), "local_bin"));
        {
            let mut st = lock_status(&core.status);
            st.gate = Gate::Active;
            st.busy = true;
            st.stalled_at = Some(4242);
        }

        let dash = core_get_dashboard(&core).expect("dashboard");
        assert_eq!(dash.gate, "active");
        assert!(dash.busy);
        assert_eq!(dash.halted, None);
        assert_eq!(dash.stalled_at, Some(4242));
        assert_eq!(dash.binary.path.as_deref(), Some("/bin/claude"));
        assert_eq!(dash.binary.source, Some("local_bin"));
        assert_eq!(dash.interval_secs, 60);
        assert_eq!(dash.accounts.len(), 1);
        assert_eq!(
            dash.accounts[0].latest.as_ref().map(|s| s.outcome),
            Some("timeout")
        );
    }

    #[test]
    fn the_dashboard_reports_the_backoff_deadline_from_the_snapshot() {
        let (tmp, core) = core();
        let dir = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &dir, 1).expect("add");

        let mut backoff = HashMap::new();
        backoff.insert(a.id.clone(), 1000 + 60_000);
        lock_status(&core.status).backoff_until = backoff;

        let dash = core_get_dashboard(&core).expect("dashboard");
        assert_eq!(dash.accounts[0].backoff_until, Some(1000 + 60_000));
    }

    #[test]
    fn an_account_with_no_entry_in_the_snapshot_has_no_backoff() {
        let (tmp, core) = core();
        let dir = make_dir(tmp.path(), ".claude3");
        core_add_account(&core, &dir, 1).expect("add");
        let dash = core_get_dashboard(&core).expect("dashboard");
        assert_eq!(dash.accounts[0].backoff_until, None);
    }

    #[test]
    fn core_reorder_accounts_persists_the_new_order() {
        let (tmp, core) = core();
        let a = core_add_account(&core, &make_dir(tmp.path(), ".claudeA"), 1).expect("a");
        let b = core_add_account(&core, &make_dir(tmp.path(), ".claudeB"), 2).expect("b");
        let c = core_add_account(&core, &make_dir(tmp.path(), ".claudeC"), 3).expect("c");

        let result = core_reorder_accounts(&core, vec![c.id.clone(), a.id.clone(), b.id.clone()])
            .expect("reorder");
        let ids: Vec<String> = result.into_iter().map(|acc| acc.id).collect();
        assert_eq!(ids, vec![c.id.clone(), a.id.clone(), b.id.clone()]);

        let relisted: Vec<String> = core
            .store
            .list_accounts()
            .expect("list")
            .into_iter()
            .map(|acc| acc.id)
            .collect();
        assert_eq!(relisted, vec![c.id, a.id, b.id]);
    }

    #[test]
    fn core_reorder_accounts_rejects_an_unknown_id() {
        let (tmp, core) = core();
        let a = core_add_account(&core, &make_dir(tmp.path(), ".claudeA"), 1).expect("a");

        let err = core_reorder_accounts(&core, vec![a.id, "nope".to_string()])
            .expect_err("must reject");
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn removing_an_account_cascades_and_rescan_adds_disabled_rows() {
        let (tmp, core) = core();
        let one = make_dir(tmp.path(), ".claude");
        let two = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &one, 1).expect("add");

        let added = core_rescan_profiles(&core, tmp.path(), 2).expect("rescan");
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].config_dir, dunce::canonicalize(&two).unwrap_or(two));
        assert!(!added[0].enabled);

        core_remove_account(&core, &a.id).expect("remove");
        assert_eq!(core.store.list_accounts().expect("list").len(), 1);
    }

    #[test]
    fn get_snapshot_raw_returns_raw_and_error() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &d, 1).expect("add");
        let id = core
            .store
            .insert_snapshot(
                &a.id,
                1,
                &PollOutcome::ParseError("missing session line".into()),
                Some("the raw report"),
                7,
            )
            .expect("snapshot");

        let got = core_get_snapshot_raw(&core, id).expect("raw");
        assert_eq!(got.raw.as_deref(), Some("the raw report"));
        assert_eq!(got.error.as_deref(), Some("missing session line"));
    }

    #[test]
    fn get_history_returns_hourly_points() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &d, 1).expect("add");
        let hour = 3_600_000i64;
        let base = 100 * hour;
        for (t, pct) in [(base + 1000, 4u8), (base + 2000, 9), (base + hour, 6)] {
            let outcome = PollOutcome::Ok(crate::usage::Parsed {
                session: crate::usage::Window { pct: 1, resets_at: None },
                week_all: crate::usage::Window { pct, resets_at: None },
                week_models: vec![],
            });
            core.store
                .insert_snapshot(&a.id, t, &outcome, None, 1)
                .expect("snapshot");
        }

        let points = core_get_history(&core, &a.id, base + hour * 8, 7).expect("history");
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].pct, 9);
        assert_eq!(points[1].pct, 6);
    }

    #[test]
    fn history_days_is_clamped_to_one_through_thirty() {
        let (tmp, core) = core();
        let d = make_dir(tmp.path(), ".claude3");
        let a = core_add_account(&core, &d, 1).expect("add");
        let hour = 3_600_000i64;
        let day = 24 * hour;
        let base = 1_000 * day;
        // One ok snapshot per listed day, each in its own hourly bucket.
        for (days_ago_from_base, pct) in [(0i64, 1u8), (10, 2), (29, 3), (31, 4)] {
            let outcome = PollOutcome::Ok(crate::usage::Parsed {
                session: crate::usage::Window { pct: 1, resets_at: None },
                week_all: crate::usage::Window { pct, resets_at: None },
                week_models: vec![],
            });
            core.store
                .insert_snapshot(&a.id, base + days_ago_from_base * day, &outcome, None, 1)
                .expect("snapshot");
        }
        // `now` is one hour past the newest snapshot (day 31), so "N days" reaches
        // back to day 31 - N + 1/24: 1 day → {31}; 7 → {29, 31}; 30 → {10, 29, 31}.
        let now = base + 31 * day + hour;
        assert_eq!(core_get_history(&core, &a.id, now, 0).expect("h").len(), 1, "0 clamps to 1 day");
        assert_eq!(core_get_history(&core, &a.id, now, 7).expect("h").len(), 2, "7 days: 29, 31");
        assert_eq!(core_get_history(&core, &a.id, now, 30).expect("h").len(), 3, "30 days: 10, 29, 31");
        assert_eq!(core_get_history(&core, &a.id, now, 999).expect("h").len(), 3, "999 clamps to 30");
    }
}
