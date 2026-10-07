//! Window-platform code behind one interface: `install` and `apply_fit`.
//! Windows gets the live aspect lock; other platforms get Tauri's portable API.

#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(windows))]
mod other;
#[cfg(not(windows))]
pub use other::*;
