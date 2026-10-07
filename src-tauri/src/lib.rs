pub mod commands;
pub mod discovery;
pub mod error;
pub mod logging;
pub mod login;
pub mod memory;
pub mod paths;
pub mod process;
pub mod scheduler;
pub mod store;
pub mod system;
#[cfg(test)]
mod test_log;
pub mod tray;
pub mod usage;
pub mod window_aspect;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::commands::{
    begin_create, lock_binary, window_created, window_destroyed, Core, SharedCore, SystemSlot,
};
use crate::memory::{MemoryProbe, RealMemoryProbe};
use crate::scheduler::driver::{
    BinaryProbe, Driver, EventSink, ProcessProbe, RealBinaryProbe, SysinfoProbe,
};
use crate::scheduler::machine::DriverStatus;
use crate::scheduler::triggers::Triggers;
use crate::store::Store;
use crate::tray::{
    apply_tray, exit_action, ExitAction, TauriEvents, MENU_LOGS, MENU_OPEN, MENU_QUIT, MENU_REFRESH,
};

/// Set once the shutdown sequence has finished, so the second
/// `ExitRequested` is allowed through.
static EXIT_APPROVED: AtomicBool = AtomicBool::new(false);

/// Set when the shutdown sequence begins, before the 2.5 s quit window. A
/// tray click or a relaunch inside it must not build a WebView2 that the
/// `process::exit` at the end would kill.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// False once shutdown has begun.
fn may_build_window(shutting_down: &AtomicBool) -> bool {
    !shutting_down.load(Ordering::SeqCst)
}

/// How long Quit waits for the driver to stop before exiting anyway.
const DRIVER_STOP_MAX: Duration = Duration::from_millis(2500);

/// How the wait for the driver's shutdown ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopOutcome {
    /// The driver finished its shutdown and signalled within the bound.
    Stopped,
    /// The driver task ended without signalling (a panic), and the whole bound
    /// was then waited out so a cycle task it no longer supervises can kill
    /// its child before the process exits.
    DriverGone,
    /// The bound elapsed first; exit proceeds anyway.
    TimedOut,
    /// No driver was ever started, so there is nothing to wait for.
    NoDriver,
}

/// Waits until the driver task signals completion, at most `max`.
pub(crate) async fn await_driver_stop(
    done: Option<oneshot::Receiver<()>>,
    max: Duration,
) -> StopOutcome {
    let Some(done) = done else {
        return StopOutcome::NoDriver;
    };
    let started = tokio::time::Instant::now();
    match tokio::time::timeout(max, done).await {
        Ok(Ok(())) => StopOutcome::Stopped,
        // The sender was dropped: the driver task ended without reaching its
        // send (a panic), so its own cycle wait never ran. A cycle task it
        // spawned is still cancelling and killing its child; give it the rest
        // of the bound rather than exiting under it.
        Ok(Err(_)) => {
            warn!(
                elapsed_ms = started.elapsed().as_millis() as u64,
                "driver task ended without stopping; waiting out the bound"
            );
            tokio::time::sleep_until(started + max).await;
            StopOutcome::DriverGone
        }
        Err(_) => StopOutcome::TimedOut,
    }
}

/// Marks shutdown as begun; true only for the first caller. The flag is set
/// before this returns, so the caller cancels the token after it is visible.
fn begin_shutdown(shutting_down: &AtomicBool) -> bool {
    !shutting_down.swap(true, Ordering::SeqCst)
}

/// What the window-state plugin persists: geometry only, never visibility
/// (D7). Used for the plugin's registration and for the explicit save when the
/// window closes to the tray, so the two cannot drift.
fn window_state_flags() -> StateFlags {
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED
}

/// Logs a failed window call at WARN and turns it into `None`.
fn ok_or_warn<T, E: std::fmt::Display>(what: &str, result: Result<T, E>) -> Option<T> {
    match result {
        Ok(v) => Some(v),
        Err(e) => {
            warn!(error = %e, "window call failed: {what}");
            None
        }
    }
}

/// Shows the main window, rebuilding it from the config when closing to the
/// tray destroyed it. Safe to call from the tray, menu and single-instance
/// callbacks: those run on the main thread, where `WebviewWindowBuilder::build`
/// deadlocks, so the build always runs on the async runtime.
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let Some(core) = app.try_state::<SharedCore>().map(|c| Arc::clone(c.inner())) else {
        error!("window create skipped: the app state is not ready");
        return;
    };
    if !may_build_window(&SHUTTING_DOWN) {
        info!("window create skipped: shutting down");
        return;
    }
    // Single flight: a second tray click or launch while a build runs is a
    // no-op. The guard moves into the task, so `creating` resets on every
    // path out of it, panics included.
    let Some(guard) = begin_create(&core) else {
        debug!("window create already running");
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = guard;
        // Quit may have begun after the pre-spawn check and before this task
        // got to run. Returning drops `_guard`, so `creating` resets.
        if !may_build_window(&SHUTTING_DOWN) {
            info!("window create skipped: shutting down");
            return;
        }
        let started = Instant::now();
        let Some(cfg) = app.config().app.windows.first().cloned() else {
            error!("window create failed: no window in the config");
            return;
        };
        let built = tauri::WebviewWindowBuilder::from_config(&app, &cfg)
            .and_then(|b| b.visible(false).build());
        match built {
            Ok(w) => {
                ok_or_warn("show", w.show());
                ok_or_warn("set_focus", w.set_focus());
                window_created(&core);
                // A failed read is omitted from the line, not logged as 0, so
                // the reopen comparison (manual M1) cannot mistake it for a
                // real reading.
                let position = ok_or_warn("outer_position", w.outer_position());
                let size = ok_or_warn("outer_size", w.outer_size());
                let maximized = ok_or_warn("is_maximized", w.is_maximized());
                info!(
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    x = position.map(|p| p.x),
                    y = position.map(|p| p.y),
                    width = size.map(|s| s.width),
                    height = size.map(|s| s.height),
                    maximized,
                    "window created"
                );
            }
            Err(e) => error!(error = %e, "window create failed"),
        }
    });
}

/// Entry point called by `main.rs`.
pub fn run() {
    let shutdown = CancellationToken::new();
    let shutdown_for_event = shutdown.clone();
    // The driver task sends on `driver_done_tx` once `Driver::run` has killed
    // and reaped its children; the exit handler waits on the receiver.
    let (driver_done_tx, driver_done_rx) = oneshot::channel::<()>();
    let mut driver_done_rx = Some(driver_done_rx);

    let result = tauri::Builder::default()
        // Single instance must be registered first so a second launch is
        // rejected before any other plugin initialises.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(window_state_flags())
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_dashboard,
            commands::get_system,
            commands::get_history,
            commands::get_history_models,
            commands::poll_now,
            commands::add_account,
            commands::update_account,
            commands::remove_account,
            commands::reorder_accounts,
            commands::rescan_profiles,
            commands::get_settings,
            commands::set_settings,
            commands::clear_halt,
            commands::open_login,
            commands::open_log_dir,
            commands::get_snapshot_raw,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            let app_data_dir = handle.path().app_data_dir()?;
            let log_dir = handle.path().app_log_dir()?;
            paths::ensure_dir(&app_data_dir)?;

            let store = Arc::new(Store::open(&paths::db_path(&app_data_dir))?);
            let stored = store.stored_settings()?;

            // Logging first, so everything below is captured.
            let log = match logging::init_logging(&log_dir, &stored.log_level) {
                Ok(h) => Some(Arc::new(h)),
                Err(e) => {
                    eprintln!("logging unavailable: {e}");
                    None
                }
            };
            info!(
                app_data_dir = %app_data_dir.display(),
                log_dir = %log_dir.display(),
                interval_secs = stored.interval_secs,
                "starting"
            );

            // Seed accounts on first start (spec 6.1).
            let home = paths::home_dir()?;
            let candidates = discovery::enumerate_profiles(&home);
            let seeded =
                store.seed_accounts_if_empty(&candidates, chrono::Utc::now().timestamp_millis())?;
            info!(seeded, discovered = candidates.len(), "accounts loaded");

            // D10: prune once at startup; the driver repeats it every 24 h.
            if let Err(e) = store.prune(chrono::Utc::now().timestamp_millis()) {
                error!(error = %e, "startup prune failed");
            }

            // The login script directory is emptied at startup (spec 6.8).
            if let Err(e) = paths::empty_dir(&paths::login_script_dir(&app_data_dir)) {
                error!(error = %e, "could not clear the login script directory");
            }

            let (settings_tx, _settings_rx) = tokio::sync::watch::channel(stored.clone());
            let core: SharedCore = Arc::new(Core {
                store,
                triggers: Arc::new(Triggers::new()),
                // The driver owns the state machine and is the only writer of
                // this snapshot (spec 5.1); everything else only reads it.
                status: Arc::new(Mutex::new(DriverStatus::default())),
                system: Arc::new(Mutex::new(SystemSlot::default())),
                binary: Arc::new(Mutex::new(None)),
                halt_latched: AtomicBool::new(false),
                // Seeded here so the window-close handler never reads the
                // store; `core_set_settings` keeps it in step.
                close_to_tray: AtomicBool::new(stored.close_to_tray),
                window_open: AtomicBool::new(true),
                creating: AtomicBool::new(false),
                sampler_kick: tokio::sync::Notify::new(),
                settings_tx,
                log,
                app_data_dir,
                log_dir,
            });
            // Resolve the binary once, before anything can be asked about
            // it. Until the driver's first `refresh_binary` the slot would
            // otherwise be empty, and a Refresh click in that window answers
            // `skipped:no_binary` even on a perfectly healthy install.
            commands::seed_binary_slot(&core.binary, &stored.claude_binary);
            info!(
                binary = ?lock_binary(&core.binary).as_ref().map(|(p, _)| p.clone()),
                "binary slot seeded"
            );

            app.manage(Arc::clone(&core));

            // Tray. `tauri.conf.json` must NOT declare `app.trayIcon`: Tauri's
            // own `Builder::build` unconditionally creates a tray from that
            // config entry (tauri-2.12.1 src/app.rs:2585, "initialize default
            // tray icon if defined") before this `setup` closure ever runs
            // (app.rs:2697 runs it, after the config windows at 2691), and
            // `AppHandle::tray_by_id` resolves the first match in insertion
            // order (src/manager/tray.rs `find_map`). A config-declared tray
            // with the same id "main" would therefore win every `tray_by_id`
            // lookup over the one built here — with no menu and no click
            // handlers — so `apply_tray` would keep updating a dead icon
            // while the real, interactive one never changes. This builder is
            // the sole creator of the tray.
            let open = MenuItem::with_id(app, MENU_OPEN, "Open", true, None::<&str>)?;
            let refresh = MenuItem::with_id(app, MENU_REFRESH, "Refresh now", true, None::<&str>)?;
            let logs = MenuItem::with_id(app, MENU_LOGS, "Open log folder", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &refresh, &logs, &quit])?;

            let menu_core = Arc::clone(&core);
            TrayIconBuilder::with_id("main")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("Claude Usage Tracker")
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    MENU_OPEN => show_main_window(app),
                    MENU_REFRESH => {
                        // `core_poll_now` reads the store, so it takes the
                        // same `blocking` hop the command wrapper takes: a
                        // contended read parks for up to `busy_timeout`, and
                        // this handler runs on the UI thread.
                        let poll_core = Arc::clone(&menu_core);
                        tauri::async_runtime::spawn(async move {
                            let result =
                                commands::blocking(move || commands::core_poll_now(&poll_core))
                                    .await;
                            match result {
                                Ok(s) => info!(result = %s, "tray refresh"),
                                Err(e) => error!(error = %e, "tray refresh failed"),
                            }
                        });
                    }
                    MENU_LOGS => {
                        use tauri_plugin_opener::OpenerExt;
                        let dir = menu_core.log_dir.to_string_lossy().to_string();
                        if let Err(e) = app.opener().open_path(dir, None::<&str>) {
                            error!(error = %e, "could not open the log folder");
                        }
                    }
                    MENU_QUIT => app.exit(0),
                    other => error!(id = other, "unhandled tray menu id"),
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            // `setup` is a sync closure, so the now-async `apply_tray`
            // (Fix round 1, item 3: one `blocking` hop instead of three
            // synchronous store calls on the setup thread) is spawned
            // rather than awaited here.
            let startup_handle = handle.clone();
            let startup_core = Arc::clone(&core);
            tauri::async_runtime::spawn(async move {
                apply_tray(&startup_handle, &startup_core).await;
            });

            // Scheduler.
            let events: Arc<dyn EventSink> =
                Arc::new(TauriEvents::new(handle.clone(), Arc::clone(&core)));
            let process: Arc<dyn ProcessProbe> = Arc::new(SysinfoProbe::new()?);
            let binary: Arc<dyn BinaryProbe> = Arc::new(RealBinaryProbe);
            let pid_slot = Arc::new(std::sync::atomic::AtomicU32::new(0));
            let memory: Arc<dyn MemoryProbe> = Arc::new(RealMemoryProbe);
            let driver = Driver::new(
                Arc::clone(&core),
                Arc::clone(&events),
                process,
                Arc::clone(&memory),
                binary,
                shutdown.clone(),
                Arc::clone(&pid_slot),
            );
            tauri::async_runtime::spawn(async move {
                driver.run().await;
                // The receiver is gone only if the app already exited.
                let _ = driver_done_tx.send(());
            });
            tauri::async_runtime::spawn(system::run_sampler(
                Arc::clone(&core),
                events,
                Arc::clone(&memory),
                pid_slot,
                shutdown.clone(),
            ));

            // The config window is created hidden (so the window-state
            // plugin restores its geometry before the first paint); show it.
            show_main_window(app.handle());

            Ok(())
        })
        .on_window_event(|window, event| {
            // A close is not intercepted: the window and its WebView2 are
            // destroyed, and `ExitRequested` (below) keeps the process alive.
            if let WindowEvent::Destroyed = event {
                if window.label() == "main" {
                    if let Some(core) = window.app_handle().try_state::<SharedCore>() {
                        window_destroyed(&core);
                    }
                    info!("window destroyed");
                }
            }
        })
        .build(tauri::generate_context!());

    let app = match result {
        Ok(a) => a,
        Err(e) => {
            eprintln!("fatal: could not build the application: {e}");
            std::process::exit(1);
        }
    };

    app.run(move |app_handle, event| {
        if let RunEvent::ExitRequested { code, api, .. } = event {
            // Atomic only: a synchronous store read here would freeze the
            // event loop for as long as the connection mutex is held.
            let close_to_tray = app_handle
                .try_state::<SharedCore>()
                .map(|c| c.close_to_tray.load(Ordering::SeqCst))
                .unwrap_or(true);
            match exit_action(code, close_to_tray, EXIT_APPROVED.load(Ordering::SeqCst)) {
                ExitAction::Allow => return,
                ExitAction::KeepRunning => {
                    api.prevent_exit();
                    info!("window closed to tray");
                    // The plugin writes geometry only on `Exit`, which a
                    // logoff or a kill may never reach. The cache already
                    // holds the final geometry (refreshed on CloseRequested),
                    // so persist it now; no window is read.
                    let handle = app_handle.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        if let Err(e) = handle.save_window_state(window_state_flags()) {
                            error!(error = %e, "window state save failed");
                        }
                    });
                    return;
                }
                ExitAction::Shutdown => {}
            }
            api.prevent_exit();
            // A second request while the first is waiting must not start a
            // second wait: the receiver is taken once.
            if !begin_shutdown(&SHUTTING_DOWN) {
                debug!("exit requested again; shutdown already running");
                return;
            }
            info!("exit requested; shutting the poller down");
            shutdown_for_event.cancel();

            let driver_done = driver_done_rx.take();
            let handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                // `app.exit()` ends in `process::exit`, which skips
                // destructors, so the driver's own shutdown path must have
                // killed and waited on any child before we get here. Wait for
                // that signal; the bound only caps a driver that hangs.
                let started = Instant::now();
                let outcome = await_driver_stop(driver_done, DRIVER_STOP_MAX).await;
                let elapsed_ms = started.elapsed().as_millis() as u64;
                if matches!(outcome, StopOutcome::TimedOut | StopOutcome::DriverGone) {
                    warn!(?outcome, elapsed_ms, "driver stop awaited");
                } else {
                    info!(?outcome, elapsed_ms, "driver stop awaited");
                }
                EXIT_APPROVED.store(true, Ordering::SeqCst);
                handle.exit(0);
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_not_rebuilt_once_shutdown_began() {
        let flag = AtomicBool::new(false);
        assert!(may_build_window(&flag));
        flag.store(true, Ordering::SeqCst);
        assert!(!may_build_window(&flag));
    }

    const MAX: Duration = Duration::from_millis(2500);

    #[tokio::test(start_paused = true)]
    async fn a_signal_sent_before_the_wait_resolves_at_once() {
        let (tx, rx) = oneshot::channel();
        tx.send(()).expect("receiver alive");
        let start = tokio::time::Instant::now();
        assert_eq!(await_driver_stop(Some(rx), MAX).await, StopOutcome::Stopped);
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn a_signal_during_the_wait_resolves_when_it_arrives() {
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = tx.send(());
        });
        let start = tokio::time::Instant::now();
        assert_eq!(await_driver_stop(Some(rx), MAX).await, StopOutcome::Stopped);
        assert_eq!(start.elapsed(), Duration::from_millis(20));
    }

    #[tokio::test(start_paused = true)]
    async fn no_signal_times_out_at_exactly_the_bound() {
        // Keep the sender alive: dropping it would count as a stop.
        let (_tx, rx) = oneshot::channel::<()>();
        let start = tokio::time::Instant::now();
        assert_eq!(
            await_driver_stop(Some(rx), MAX).await,
            StopOutcome::TimedOut
        );
        assert_eq!(start.elapsed(), MAX);
    }

    #[tokio::test(start_paused = true)]
    async fn no_receiver_means_no_driver_and_no_wait() {
        let start = tokio::time::Instant::now();
        assert_eq!(await_driver_stop(None, MAX).await, StopOutcome::NoDriver);
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn a_dropped_sender_waits_out_the_whole_bound() {
        // A panicked driver no longer supervises its cycle task, which still
        // needs time to kill its child; the exit must not run ahead of it.
        let (tx, rx) = oneshot::channel::<()>();
        drop(tx);
        let start = tokio::time::Instant::now();
        assert_eq!(
            await_driver_stop(Some(rx), MAX).await,
            StopOutcome::DriverGone
        );
        assert_eq!(start.elapsed(), MAX);
    }

    #[tokio::test(start_paused = true)]
    async fn a_sender_dropped_mid_wait_still_ends_at_the_bound_from_the_start() {
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            drop(tx);
        });
        let start = tokio::time::Instant::now();
        assert_eq!(
            await_driver_stop(Some(rx), MAX).await,
            StopOutcome::DriverGone
        );
        assert_eq!(start.elapsed(), MAX);
    }

    #[test]
    fn the_first_shutdown_request_wins_and_sets_the_flag() {
        let flag = AtomicBool::new(false);
        assert!(begin_shutdown(&flag));
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn a_repeat_shutdown_request_is_refused() {
        let flag = AtomicBool::new(false);
        assert!(begin_shutdown(&flag));
        assert!(!begin_shutdown(&flag));
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn a_failed_window_call_is_logged_and_becomes_none() {
        let log = crate::test_log::captured(|| {
            assert_eq!(ok_or_warn("show", Ok::<u8, String>(7)), Some(7));
            assert_eq!(
                ok_or_warn("outer_size", Err::<u8, String>("gone".into())),
                None
            );
        });
        assert!(log.contains("WARN"), "{log}");
        assert!(log.contains("outer_size"), "{log}");
        assert!(log.contains("gone"), "{log}");
        assert!(!log.contains("show"), "a success must not log: {log}");
    }

    #[test]
    fn the_saved_window_state_is_geometry_only() {
        // D7: never the visible flag, which would restore a closed-to-tray
        // window as shown.
        let flags = window_state_flags();
        assert!(flags.contains(StateFlags::SIZE));
        assert!(flags.contains(StateFlags::POSITION));
        assert!(flags.contains(StateFlags::MAXIMIZED));
        assert!(!flags.contains(StateFlags::VISIBLE));
    }
}
