use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;

use cut_core::commands::{Core, SystemSlot};
use cut_core::scheduler::machine::DriverStatus;
use cut_core::scheduler::triggers::Triggers;
use cut_core::store::settings::UserSettings;
use cut_core::store::Store;

pub fn defaults() -> UserSettings {
    UserSettings {
        interval_secs: 10,
        timeout_secs: 5,
        claude_binary: String::new(),
        close_to_tray: true,
        launch_at_login: false,
        log_level: "info".to_string(),
    }
}

pub fn test_core(
    dir: &std::path::Path,
) -> (Arc<Core>, tokio::sync::watch::Receiver<UserSettings>) {
    let store = Arc::new(Store::open_in_memory().expect("open"));
    let (settings_tx, settings_rx) = tokio::sync::watch::channel(defaults());
    store.save_settings(&defaults()).expect("save settings");
    let core = Arc::new(Core {
        store,
        triggers: Arc::new(Triggers::new()),
        status: Arc::new(Mutex::new(DriverStatus::default())),
        system: Arc::new(Mutex::new(SystemSlot::default())),
        binary: Arc::new(Mutex::new(None)),
        halt_latched: AtomicBool::new(false),
        close_to_tray: AtomicBool::new(true),
        settings_tx,
        log: None,
        app_data_dir: dir.to_path_buf(),
        log_dir: dir.join("logs"),
    });
    (core, settings_rx)
}
