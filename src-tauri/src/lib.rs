pub mod discovery;
pub mod error;
pub mod login;
pub mod logging;
pub mod paths;
pub mod process;
pub mod scheduler;
pub mod store;
pub mod tray;
pub mod usage;

/// Entry point called by `main.rs`. Fully wired in Task 20.
pub fn run() {
    if let Err(e) = tauri::Builder::default().run(tauri::generate_context!()) {
        eprintln!("fatal: failed to run tauri application: {e}");
        std::process::exit(1);
    }
}
