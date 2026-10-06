use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use cut_core::commands::{Core, SystemSlot};
use cut_core::memory::MemoryProbe;
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
        min_free_memory_mb: 1536,
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
        window_open: AtomicBool::new(true),
        creating: AtomicBool::new(false),
        sampler_kick: tokio::sync::Notify::new(),
        settings_tx,
        log: None,
        app_data_dir: dir.to_path_buf(),
        log_dir: dir.join("logs"),
    });
    (core, settings_rx)
}

/// A scripted commit-headroom probe. Reads `Some(u64::MAX)` until a test
/// scripts it, so no test depends on this machine's free commit.
pub struct FakeMemory(Mutex<VecDeque<Option<u64>>>);

impl FakeMemory {
    pub fn new() -> FakeMemory {
        FakeMemory(Mutex::new(VecDeque::from([Some(u64::MAX)])))
    }

    /// Replaces the queue. Each read pops one reading and repeats the last.
    pub fn script(&self, readings: &[Option<u64>]) {
        let mut q = self.0.lock().unwrap_or_else(|p| p.into_inner());
        *q = readings.iter().copied().collect();
        if q.is_empty() {
            q.push_back(Some(u64::MAX));
        }
    }
}

impl MemoryProbe for FakeMemory {
    fn available_commit_bytes(&self) -> Option<u64> {
        let mut q = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if q.len() > 1 {
            q.pop_front().flatten()
        } else {
            q.front().copied().flatten()
        }
    }
}
