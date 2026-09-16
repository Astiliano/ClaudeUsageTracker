use crate::usage::{Account, SnapshotDto};

/// D9: the worst percentage across every enabled account and every window.
/// `Halted` is a distinct level that takes precedence over all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Halted,
    Grey,
    Green,
    Amber,
    Red,
}

impl Level {
    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Halted => "halted",
            Level::Grey => "grey",
            Level::Green => "green",
            Level::Amber => "amber",
            Level::Red => "red",
        }
    }

    fn from_pct(pct: u8) -> Level {
        match pct {
            0..=69 => Level::Green,
            70..=89 => Level::Amber,
            _ => Level::Red,
        }
    }
}

/// The largest percentage across every window of one `ok` snapshot.
fn worst_pct(dto: &SnapshotDto) -> Option<u8> {
    if dto.outcome != "ok" {
        return None;
    }
    let mut worst: Option<u8> = None;
    let mut consider = |p: u8| {
        worst = Some(worst.map_or(p, |w: u8| w.max(p)));
    };
    if let Some(w) = dto.session {
        consider(w.pct);
    }
    if let Some(w) = dto.week_all {
        consider(w.pct);
    }
    for m in &dto.week_models {
        consider(m.pct);
    }
    worst
}

fn tooltip_line(account: &Account, dto: Option<&SnapshotDto>) -> String {
    let dto = match dto {
        Some(d) if d.outcome == "ok" => d,
        _ => return format!("{}  err", account.label),
    };
    let mut segments: Vec<String> = Vec::new();
    if let Some(w) = dto.session {
        segments.push(format!("S {}%", w.pct));
    }
    if let Some(w) = dto.week_all {
        segments.push(format!("W {}%", w.pct));
    }
    for m in &dto.week_models {
        segments.push(format!("{} {}%", m.label, m.pct));
    }
    if segments.is_empty() {
        return format!("{}  err", account.label);
    }
    format!("{}  {}", account.label, segments.join(" · "))
}

/// Pure: level plus tooltip for the tray icon. `halted` is read from the same
/// store key `get_dashboard` reports.
pub fn tray_state(latest: &[(Account, Option<SnapshotDto>)], halted: bool) -> (Level, String) {
    if halted {
        return (Level::Halted, "polling halted — guard tripped".to_string());
    }

    let enabled: Vec<&(Account, Option<SnapshotDto>)> =
        latest.iter().filter(|(a, _)| a.enabled).collect();
    if enabled.is_empty() {
        return (Level::Grey, "no enabled accounts".to_string());
    }

    let worst = enabled
        .iter()
        .filter_map(|(_, d)| d.as_ref().and_then(worst_pct))
        .max();
    let level = match worst {
        Some(p) => Level::from_pct(p),
        None => Level::Grey,
    };

    let tooltip = enabled
        .iter()
        .map(|(a, d)| tooltip_line(a, d.as_ref()))
        .collect::<Vec<String>>()
        .join("\n");

    (level, tooltip)
}

pub const MENU_OPEN: &str = "open";
pub const MENU_REFRESH: &str = "refresh_now";
pub const MENU_LOGS: &str = "open_log_folder";
pub const MENU_QUIT: &str = "quit";

/// Icons are drawn rather than shipped as assets, so the five states stay in
/// sync with `Level` and there is nothing to keep in a bundle.
pub fn level_rgb(level: Level) -> [u8; 3] {
    match level {
        Level::Halted => [0x8B, 0x1A, 0x1A],
        Level::Grey => [0x8A, 0x8A, 0x8A],
        Level::Green => [0x2E, 0xA0, 0x43],
        Level::Amber => [0xD2, 0x96, 0x22],
        Level::Red => [0xD7, 0x33, 0x33],
    }
}

/// A filled circle in the level colour, plus a small opaque badge in the
/// top-right quadrant for `Halted` so the halted state is distinguishable at
/// tray size even in monochrome.
pub fn icon_rgba(level: Level, size: u32) -> Vec<u8> {
    let rgb = level_rgb(level);
    let mut buf = vec![0u8; (size * size * 4) as usize];
    let centre = size as f32 / 2.0;
    let radius = centre - 1.0;
    let badge_centre = (size as f32 * 0.8, size as f32 * 0.2);
    let badge_radius = size as f32 * 0.22;

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let dx = x as f32 + 0.5 - centre;
            let dy = y as f32 + 0.5 - centre;

            let bdx = x as f32 + 0.5 - badge_centre.0;
            let bdy = y as f32 + 0.5 - badge_centre.1;
            let in_badge_slot = bdx * bdx + bdy * bdy <= badge_radius * badge_radius;
            // The badge slot is cut out of the circle for every level, not
            // only `Halted`: otherwise the circle's own fill bleeds into the
            // slot for the other four levels and the badge stops being a
            // reliable "this is Halted" signal.
            let inside = dx * dx + dy * dy <= radius * radius && !in_badge_slot;
            let in_badge = level == Level::Halted && in_badge_slot;

            if in_badge {
                buf[idx] = 0xFF;
                buf[idx + 1] = 0xCC;
                buf[idx + 2] = 0x00;
                buf[idx + 3] = 255;
            } else if inside {
                buf[idx] = rgb[0];
                buf[idx + 1] = rgb[1];
                buf[idx + 2] = rgb[2];
                buf[idx + 3] = 255;
            }
        }
    }
    buf
}

/// Window close hides to the tray unless the user turned that off.
pub fn should_hide_on_close(close_to_tray: bool) -> bool {
    close_to_tray
}

use tauri::image::Image;
use tracing::warn;

use crate::commands::Core;

const TRAY_ICON_SIZE: u32 = 32;

/// Recomputes the level and tooltip from the store and pushes them onto the
/// tray icon. Called after each account's poll and on account/settings change.
pub fn apply_tray(app: &tauri::AppHandle, core: &Core) {
    let accounts = match core.store.list_accounts() {
        Ok(a) => a,
        Err(e) => {
            warn!(error = %e, "could not read accounts for the tray");
            return;
        }
    };
    let latest = core.store.latest_per_account().unwrap_or_default();
    let halted = core
        .store
        .polling_halted()
        .unwrap_or_default()
        .is_some();

    let rows: Vec<(Account, Option<SnapshotDto>)> = accounts
        .into_iter()
        .map(|a| {
            let snap = latest.get(&a.id).cloned();
            (a, snap)
        })
        .collect();

    let (level, tooltip) = tray_state(&rows, halted);

    let tray = match app.tray_by_id("main") {
        Some(t) => t,
        None => {
            warn!("tray icon 'main' not found");
            return;
        }
    };

    let rgba = icon_rgba(level, TRAY_ICON_SIZE);
    let image = Image::new_owned(rgba, TRAY_ICON_SIZE, TRAY_ICON_SIZE);
    if let Err(e) = tray.set_icon(Some(image)) {
        warn!(error = %e, "could not set the tray icon");
    }
    if let Err(e) = tray.set_tooltip(Some(&tooltip)) {
        warn!(error = %e, "could not set the tray tooltip");
    }
}

use serde::Serialize;
use std::sync::Arc;
use tauri::Emitter;

use crate::scheduler::driver::EventSink;

#[derive(Serialize, Clone)]
struct AccountEvent<'a> {
    account_id: &'a str,
}

#[derive(Serialize, Clone)]
struct GateEvent<'a> {
    gate: &'a str,
}

#[derive(Serialize, Clone)]
struct StalledEvent {
    at: i64,
    cycle_age_ms: u64,
}

/// Events are refetch triggers only: the frontend ignores the payloads and
/// re-reads state through commands. The payloads exist for logs and tests.
pub struct TauriEvents {
    app: tauri::AppHandle,
    core: Arc<Core>,
}

impl TauriEvents {
    pub fn new(app: tauri::AppHandle, core: Arc<Core>) -> TauriEvents {
        TauriEvents { app, core }
    }
}

impl EventSink for TauriEvents {
    fn usage_updated(&self, account_id: &str) {
        let _ = self.app.emit("usage:updated", AccountEvent { account_id });
    }
    fn cycle_finished(&self) {
        let _ = self.app.emit("cycle:finished", ());
    }
    fn gate_changed(&self, gate: &str) {
        let _ = self.app.emit("gate:changed", GateEvent { gate });
    }
    fn poller_stalled(&self, at: i64, cycle_age_ms: u64) {
        let _ = self
            .app
            .emit("poller:stalled", StalledEvent { at, cycle_age_ms });
    }
    fn refresh_tray(&self) {
        apply_tray(&self.app, &self.core);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{ModelWindow, SnapshotDto, Window};

    fn account(label: &str, enabled: bool) -> Account {
        Account {
            id: format!("id-{label}"),
            label: label.to_string(),
            config_dir: std::path::PathBuf::from(format!("/home/josh/.{label}")),
            enabled,
            disabled_reason: None,
            is_default: false,
            created_at: 0,
        }
    }

    fn ok_snapshot(session: u8, week: u8, models: &[(&str, u8)]) -> SnapshotDto {
        SnapshotDto {
            id: 1,
            account_id: "x".into(),
            taken_at: 0,
            outcome: "ok",
            session: Some(Window { pct: session, resets_at: None }),
            week_all: Some(Window { pct: week, resets_at: None }),
            week_models: models
                .iter()
                .map(|(l, p)| ModelWindow {
                    label: (*l).to_string(),
                    pct: *p,
                    resets_at: None,
                })
                .collect(),
            error: None,
            duration_ms: 1,
        }
    }

    fn failed_snapshot(outcome: &'static str) -> SnapshotDto {
        SnapshotDto {
            id: 2,
            account_id: "x".into(),
            taken_at: 0,
            outcome,
            session: None,
            week_all: None,
            week_models: vec![],
            error: Some("boom".into()),
            duration_ms: 1,
        }
    }

    #[test]
    fn level_wire_forms_are_snake_case() {
        assert_eq!(Level::Halted.as_str(), "halted");
        assert_eq!(Level::Grey.as_str(), "grey");
        assert_eq!(Level::Green.as_str(), "green");
        assert_eq!(Level::Amber.as_str(), "amber");
        assert_eq!(Level::Red.as_str(), "red");
    }

    #[test]
    fn halted_beats_everything_including_a_healthy_account() {
        let rows = vec![(account("claude", true), Some(ok_snapshot(1, 1, &[])))];
        let (level, tooltip) = tray_state(&rows, true);
        assert_eq!(level, Level::Halted);
        assert_eq!(tooltip, "polling halted — guard tripped");
    }

    #[test]
    fn no_ok_snapshot_anywhere_is_grey() {
        let rows = vec![
            (account("claude", true), None),
            (account("claude3", true), Some(failed_snapshot("timeout"))),
        ];
        let (level, _) = tray_state(&rows, false);
        assert_eq!(level, Level::Grey);
    }

    #[test]
    fn no_enabled_accounts_at_all_is_grey() {
        let rows = vec![(account("claude", false), Some(ok_snapshot(99, 99, &[])))];
        let (level, _) = tray_state(&rows, false);
        assert_eq!(level, Level::Grey);
    }

    #[test]
    fn thresholds_are_green_below_seventy_amber_to_eighty_nine_red_from_ninety() {
        for (pct, want) in [
            (0u8, Level::Green),
            (69, Level::Green),
            (70, Level::Amber),
            (89, Level::Amber),
            (90, Level::Red),
            (100, Level::Red),
        ] {
            let rows = vec![(account("claude", true), Some(ok_snapshot(pct, 0, &[])))];
            let (level, _) = tray_state(&rows, false);
            assert_eq!(level, want, "pct {pct}");
        }
    }

    #[test]
    fn the_worst_window_across_every_account_wins() {
        let rows = vec![
            (account("claude", true), Some(ok_snapshot(10, 20, &[("Fable", 30)]))),
            (account("claude3", true), Some(ok_snapshot(5, 5, &[("Opus", 95)]))),
        ];
        let (level, _) = tray_state(&rows, false);
        assert_eq!(level, Level::Red, "a per-model line counts too");
    }

    #[test]
    fn a_disabled_account_is_ignored_for_both_level_and_tooltip() {
        let rows = vec![
            (account("claude", true), Some(ok_snapshot(10, 10, &[]))),
            (account("claude-old", false), Some(ok_snapshot(99, 99, &[]))),
        ];
        let (level, tooltip) = tray_state(&rows, false);
        assert_eq!(level, Level::Green);
        assert!(!tooltip.contains("claude-old"));
    }

    #[test]
    fn the_tooltip_lists_one_line_per_enabled_account() {
        let rows = vec![
            (account("claude", true), Some(ok_snapshot(15, 4, &[("Fable", 5)]))),
            (account("claude3", true), Some(ok_snapshot(2, 1, &[]))),
        ];
        let (_, tooltip) = tray_state(&rows, false);
        assert_eq!(
            tooltip,
            "claude  S 15% · W 4% · Fable 5%\nclaude3  S 2% · W 1%"
        );
    }

    #[test]
    fn several_per_model_segments_are_repeated_by_label() {
        let rows = vec![(
            account("claude", true),
            Some(ok_snapshot(15, 4, &[("Fable", 5), ("Opus", 12)])),
        )];
        let (_, tooltip) = tray_state(&rows, false);
        assert_eq!(tooltip, "claude  S 15% · W 4% · Fable 5% · Opus 12%");
    }

    #[test]
    fn a_failing_account_renders_as_err() {
        let rows = vec![
            (account("claude", true), Some(ok_snapshot(15, 4, &[]))),
            (account("claude3", true), Some(failed_snapshot("spawn_error"))),
        ];
        let (_, tooltip) = tray_state(&rows, false);
        assert_eq!(tooltip, "claude  S 15% · W 4%\nclaude3  err");
    }

    #[test]
    fn an_account_with_no_snapshot_yet_renders_as_err() {
        let rows = vec![(account("claude", true), None)];
        let (_, tooltip) = tray_state(&rows, false);
        assert_eq!(tooltip, "claude  err");
    }

    #[test]
    fn an_empty_account_list_is_grey_with_an_explanatory_tooltip() {
        let (level, tooltip) = tray_state(&[], false);
        assert_eq!(level, Level::Grey);
        assert_eq!(tooltip, "no enabled accounts");
    }

    #[test]
    fn menu_ids_are_stable_snake_case_strings() {
        assert_eq!(MENU_OPEN, "open");
        assert_eq!(MENU_REFRESH, "refresh_now");
        assert_eq!(MENU_LOGS, "open_log_folder");
        assert_eq!(MENU_QUIT, "quit");
    }

    #[test]
    fn every_level_has_a_distinct_colour() {
        let colours = [
            level_rgb(Level::Halted),
            level_rgb(Level::Grey),
            level_rgb(Level::Green),
            level_rgb(Level::Amber),
            level_rgb(Level::Red),
        ];
        for (i, a) in colours.iter().enumerate() {
            for (j, b) in colours.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "levels {i} and {j} must look different");
                }
            }
        }
    }

    #[test]
    fn the_icon_is_a_square_rgba_buffer() {
        let size = 32u32;
        let buf = icon_rgba(Level::Green, size);
        assert_eq!(buf.len(), (size * size * 4) as usize);
    }

    #[test]
    fn the_icon_centre_carries_the_level_colour_at_full_opacity() {
        let size = 32u32;
        let buf = icon_rgba(Level::Red, size);
        let centre = ((size / 2 * size) + size / 2) as usize * 4;
        assert_eq!(&buf[centre..centre + 3], &level_rgb(Level::Red));
        assert_eq!(buf[centre + 3], 255);
    }

    #[test]
    fn the_icon_corners_are_transparent() {
        let size = 32u32;
        let buf = icon_rgba(Level::Green, size);
        assert_eq!(buf[3], 0, "top-left pixel must be transparent");
        let last = ((size * size - 1) * 4 + 3) as usize;
        assert_eq!(buf[last], 0, "bottom-right pixel must be transparent");
    }

    #[test]
    fn the_halted_icon_carries_a_badge_the_others_do_not() {
        let size = 32u32;
        let halted = icon_rgba(Level::Halted, size);
        let green = icon_rgba(Level::Green, size);
        // The badge sits in the top-right quadrant.
        let badge = ((size / 5 * size) + (size * 4 / 5)) as usize * 4;
        assert_eq!(halted[badge + 3], 255, "the halted badge must be opaque");
        assert_eq!(green[badge + 3], 0, "other levels must have no badge");
    }

    #[test]
    fn close_to_tray_decides_whether_a_window_close_hides_or_quits() {
        assert!(should_hide_on_close(true));
        assert!(!should_hide_on_close(false));
    }
}
