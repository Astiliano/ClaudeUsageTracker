# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Memory floor setting (`min_free_memory_mb`, default 1536 MB of free commit,
  0 = never hold, maximum 65536). Automatic refreshes are held below the floor
  and resume when headroom recovers; Refresh now, startup and account changes
  bypass it. The header shows a `held · <n> free` chip while a hold is active.
- Per-cycle peak memory of the polling child (`peak_working_set_bytes`,
  `peak_commit_bytes`) in the log.
- `tauri-plugin-window-state`: the window returns at its last size, position and
  maximized state.
- The history chart, rings and sparkline scale with the window.

### Changed

- Updated to tauri 2.12 (includes the wry 0.56.1 WebView2 teardown fix).
- Closing to the tray destroys the window to free its WebView2 memory; a tray
  click or a second launch rebuilds it.
- The system sampler runs every 30 s while the window is hidden (5 s while it
  is open).
- The UI uses the system font stack.
- Denser, flat layout: 6 px gutters, 40 px rows, square panels with 1 px lines;
  the table and card breakpoints are computed from the layout constants (757 px
  and 573 px).

### Removed

- The font picker and the three `@fontsource` packages.
