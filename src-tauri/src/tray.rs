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
}
