use chrono::{DateTime, Datelike, Duration, LocalResult, TimeZone, Utc};
use chrono_tz::Tz;
use regex::Regex;
use std::sync::OnceLock;

use super::{Parsed, PollOutcome, Window};

struct Patterns {
    detect: Regex,
    session: Regex,
    week_all: Regex,
    week_model: Regex,
    clause: Regex,
}

/// Compiled once. Returns `None` only if a literal pattern fails to compile,
/// which the caller turns into a parse error rather than a panic.
fn patterns() -> Option<&'static Patterns> {
    static P: OnceLock<Option<Patterns>> = OnceLock::new();
    P.get_or_init(|| {
        Some(Patterns {
            detect: Regex::new(r"^Current (session|week)\b").ok()?,
            session: Regex::new(
                r"^Current session: (\d{1,3})% used(?: · resets (.+?) \((.+)\))?$",
            )
            .ok()?,
            week_all: Regex::new(
                r"^Current week \(all models\): (\d{1,3})% used(?: · resets (.+?) \((.+)\))?$",
            )
            .ok()?,
            week_model: Regex::new(
                r"^Current week \((.+?)\): (\d{1,3})% used(?: · resets (.+?) \((.+)\))?$",
            )
            .ok()?,
            clause: Regex::new(r"^([A-Z][a-z]{2}) (\d{1,2}), (\d{1,2})(?::(\d{2}))?(am|pm)$")
                .ok()?,
        })
    })
    .as_ref()
}

fn month_from_abbrev(a: &str) -> Option<u32> {
    match a {
        "Jan" => Some(1),
        "Feb" => Some(2),
        "Mar" => Some(3),
        "Apr" => Some(4),
        "May" => Some(5),
        "Jun" => Some(6),
        "Jul" => Some(7),
        "Aug" => Some(8),
        "Sep" => Some(9),
        "Oct" => Some(10),
        "Nov" => Some(11),
        "Dec" => Some(12),
        _ => None,
    }
}

/// Resolve a wall-clock local time in `tz` to a UTC instant.
/// DST gap: first valid instant after the gap. Ambiguous: earliest.
fn resolve_local(tz: Tz, y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Option<DateTime<Utc>> {
    match tz.with_ymd_and_hms(y, mo, d, h, mi, 0) {
        LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(earliest, _latest) => Some(earliest.with_timezone(&Utc)),
        LocalResult::None => {
            let start = h * 60 + mi;
            for step in 1..=180u32 {
                let total = start + step;
                if total >= 24 * 60 {
                    return None;
                }
                match tz.with_ymd_and_hms(y, mo, d, total / 60, total % 60, 0) {
                    LocalResult::Single(dt) => return Some(dt.with_timezone(&Utc)),
                    LocalResult::Ambiguous(earliest, _) => {
                        return Some(earliest.with_timezone(&Utc))
                    }
                    LocalResult::None => continue,
                }
            }
            None
        }
    }
}

/// Parse `Sep 16, 3:30am` plus `America/Los_Angeles` into epoch ms UTC.
fn parse_reset(p: &Patterns, clause: &str, zone: &str, now: DateTime<Utc>) -> Result<i64, String> {
    let c = p
        .clause
        .captures(clause)
        .ok_or_else(|| format!("bad reset clause: {clause}"))?;
    let month = month_from_abbrev(&c[1]).ok_or_else(|| format!("bad month: {}", &c[1]))?;
    let day: u32 = c[2].parse().map_err(|_| format!("bad day: {}", &c[2]))?;
    let hour12: u32 = c[3].parse().map_err(|_| format!("bad hour: {}", &c[3]))?;
    if !(1..=12).contains(&hour12) {
        return Err(format!("hour out of range: {hour12}"));
    }
    let minute: u32 = match c.get(4) {
        Some(m) => m
            .as_str()
            .parse()
            .map_err(|_| format!("bad minute: {}", m.as_str()))?,
        None => 0,
    };
    if minute > 59 {
        return Err(format!("minute out of range: {minute}"));
    }
    let hour = match (hour12, &c[5]) {
        (12, "am") => 0,
        (12, _) => 12,
        (h, "pm") => h + 12,
        (h, _) => h,
    };

    let tz: Tz = zone.parse().map_err(|_| format!("unknown zone: {zone}"))?;
    let year = now.with_timezone(&tz).year();
    let first = resolve_local(tz, year, month, day, hour, minute)
        .ok_or_else(|| format!("invalid local time: {clause} ({zone})"))?;
    if first < now - Duration::days(30) {
        let next = resolve_local(tz, year + 1, month, day, hour, minute)
            .ok_or_else(|| format!("invalid local time: {clause} ({zone})"))?;
        return Ok(next.timestamp_millis());
    }
    Ok(first.timestamp_millis())
}

fn pct_from(raw: &str) -> Result<u8, String> {
    let n: u16 = raw.parse().map_err(|_| format!("bad pct: {raw}"))?;
    if n > 100 {
        return Err("pct out of range".to_string());
    }
    Ok(n as u8)
}

/// Build a `Window` from a pct capture plus the optional reset captures.
fn window_from(
    p: &Patterns,
    pct_raw: &str,
    clause: Option<&str>,
    zone: Option<&str>,
    now: DateTime<Utc>,
) -> Result<Window, String> {
    let pct = pct_from(pct_raw)?;
    let resets_at = match (clause, zone) {
        (Some(c), Some(z)) => Some(parse_reset(p, c, z, now)?),
        _ => None,
    };
    Ok(Window { pct, resets_at })
}

/// Parse the `/usage` report text. Pure: every clock input arrives as `now`.
pub fn parse_usage(result_text: &str, now: DateTime<Utc>) -> PollOutcome {
    let p = match patterns() {
        Some(p) => p,
        None => return PollOutcome::ParseError("internal: regex compilation failed".into()),
    };

    let lines: Vec<&str> = result_text.lines().map(|l| l.trim_end()).collect();

    if !lines.iter().any(|l| p.detect.is_match(l)) {
        return PollOutcome::NoUsageData;
    }

    let mut session: Option<Window> = None;
    let mut week_all: Option<Window> = None;
    let mut week_models: Vec<(String, Window)> = Vec::new();

    for line in &lines {
        if let Some(c) = p.session.captures(line) {
            if session.is_some() {
                return PollOutcome::ParseError("duplicate session line".into());
            }
            match window_from(
                p,
                &c[1],
                c.get(2).map(|m| m.as_str()),
                c.get(3).map(|m| m.as_str()),
                now,
            ) {
                Ok(w) => session = Some(w),
                Err(e) => return PollOutcome::ParseError(e),
            }
            continue;
        }
        if let Some(c) = p.week_all.captures(line) {
            if week_all.is_some() {
                return PollOutcome::ParseError("duplicate week (all models) line".into());
            }
            match window_from(
                p,
                &c[1],
                c.get(2).map(|m| m.as_str()),
                c.get(3).map(|m| m.as_str()),
                now,
            ) {
                Ok(w) => week_all = Some(w),
                Err(e) => return PollOutcome::ParseError(e),
            }
            continue;
        }
        if let Some(c) = p.week_model.captures(line) {
            let label = c[1].to_string();
            match window_from(
                p,
                &c[2],
                c.get(3).map(|m| m.as_str()),
                c.get(4).map(|m| m.as_str()),
                now,
            ) {
                Ok(w) => week_models.push((label, w)),
                Err(e) => return PollOutcome::ParseError(e),
            }
            continue;
        }
        // Unknown line: ignored for forward compatibility.
    }

    let session = match session {
        Some(w) => w,
        None => return PollOutcome::ParseError("missing session line".into()),
    };
    let week_all = match week_all {
        Some(w) => w,
        None => return PollOutcome::ParseError("missing week (all models) line".into()),
    };

    PollOutcome::Ok(Parsed {
        session,
        week_all,
        week_models,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<chrono::Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0)
            .single()
            .expect("fixed test instant must be valid")
    }

    fn ms(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        at(y, mo, d, h, mi).timestamp_millis()
    }

    fn now_sep_2026() -> chrono::DateTime<chrono::Utc> {
        at(2026, 9, 15, 20, 0)
    }

    fn parsed(text: &str, now: chrono::DateTime<chrono::Utc>) -> Parsed {
        match parse_usage(text, now) {
            PollOutcome::Ok(p) => p,
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn full_report_parses_all_three_lines() {
        let p = parsed(
            include_str!("../../tests/fixtures/full_report.txt"),
            now_sep_2026(),
        );
        assert_eq!(p.session.pct, 15);
        // Sep 16 2026 03:30 America/Los_Angeles (PDT, UTC-7) == 10:30 UTC.
        assert_eq!(p.session.resets_at, Some(ms(2026, 9, 16, 10, 30)));
        assert_eq!(p.week_all.pct, 4);
        // Sep 21 2026 08:00 PDT == 15:00 UTC.
        assert_eq!(p.week_all.resets_at, Some(ms(2026, 9, 21, 15, 0)));
        assert_eq!(p.week_models.len(), 1);
        assert_eq!(p.week_models[0].0, "Fable");
        assert_eq!(p.week_models[0].1.pct, 5);
        assert_eq!(p.week_models[0].1.resets_at, Some(ms(2026, 9, 21, 15, 0)));
    }

    #[test]
    fn zero_percent_session_without_a_reset_clause() {
        let p = parsed(
            include_str!("../../tests/fixtures/session_zero_no_reset.txt"),
            now_sep_2026(),
        );
        assert_eq!(p.session.pct, 0);
        assert_eq!(p.session.resets_at, None);
        assert_eq!(p.week_all.pct, 4);
    }

    #[test]
    fn not_logged_in_cost_summary_is_no_usage_data() {
        let out = parse_usage(
            include_str!("../../tests/fixtures/not_logged_in.txt"),
            now_sep_2026(),
        );
        assert_eq!(out, PollOutcome::NoUsageData);
    }

    #[test]
    fn two_per_model_lines_keep_output_order() {
        let p = parsed(
            include_str!("../../tests/fixtures/two_per_model.txt"),
            now_sep_2026(),
        );
        assert_eq!(p.week_models.len(), 2);
        assert_eq!(p.week_models[0].0, "Fable");
        assert_eq!(p.week_models[0].1.pct, 5);
        assert_eq!(p.week_models[1].0, "Opus");
        assert_eq!(p.week_models[1].1.pct, 12);
    }

    #[test]
    fn zero_per_model_lines_is_fine() {
        let p = parsed(
            include_str!("../../tests/fixtures/no_per_model.txt"),
            now_sep_2026(),
        );
        assert!(p.week_models.is_empty());
    }

    #[test]
    fn r3_never_steals_the_all_models_line() {
        let p = parsed(
            include_str!("../../tests/fixtures/full_report.txt"),
            now_sep_2026(),
        );
        assert!(
            p.week_models.iter().all(|(l, _)| l != "all models"),
            "the all-models line must never appear in week_models"
        );
    }

    #[test]
    fn missing_session_line_is_a_parse_error() {
        let out = parse_usage(
            include_str!("../../tests/fixtures/missing_session.txt"),
            now_sep_2026(),
        );
        assert_eq!(out, PollOutcome::ParseError("missing session line".into()));
    }

    #[test]
    fn missing_week_all_line_is_a_parse_error() {
        let out = parse_usage(
            include_str!("../../tests/fixtures/missing_week_all.txt"),
            now_sep_2026(),
        );
        assert_eq!(
            out,
            PollOutcome::ParseError("missing week (all models) line".into())
        );
    }

    #[test]
    fn pct_over_one_hundred_is_a_parse_error() {
        let out = parse_usage(
            include_str!("../../tests/fixtures/pct_out_of_range.txt"),
            now_sep_2026(),
        );
        assert_eq!(out, PollOutcome::ParseError("pct out of range".into()));
    }

    #[test]
    fn non_numeric_pct_fails_the_line_and_then_the_report() {
        // The line matches no regex, so it is ignored for forward
        // compatibility; the resulting missing required line is what surfaces.
        let out = parse_usage(
            include_str!("../../tests/fixtures/non_numeric_pct.txt"),
            now_sep_2026(),
        );
        assert_eq!(out, PollOutcome::ParseError("missing session line".into()));
    }

    #[test]
    fn duplicate_session_line_is_a_parse_error() {
        let out = parse_usage(
            include_str!("../../tests/fixtures/duplicate_session.txt"),
            now_sep_2026(),
        );
        assert_eq!(out, PollOutcome::ParseError("duplicate session line".into()));
    }

    #[test]
    fn unknown_zone_is_a_parse_error() {
        let out = parse_usage(
            include_str!("../../tests/fixtures/unknown_zone.txt"),
            now_sep_2026(),
        );
        assert_eq!(
            out,
            PollOutcome::ParseError("unknown zone: Mars/Olympus_Mons".into())
        );
    }

    #[test]
    fn unknown_extra_lines_are_ignored() {
        let p = parsed(
            include_str!("../../tests/fixtures/unknown_extra_lines.txt"),
            now_sep_2026(),
        );
        assert_eq!(p.session.pct, 15);
        assert_eq!(p.week_all.pct, 4);
        assert!(p.week_models.is_empty());
    }

    #[test]
    fn crlf_input_parses() {
        let p = parsed(include_str!("../../tests/fixtures/crlf.txt"), now_sep_2026());
        assert_eq!(p.session.pct, 15);
        assert_eq!(p.week_all.pct, 4);
        assert_eq!(p.session.resets_at, Some(ms(2026, 9, 16, 10, 30)));
    }

    #[test]
    fn twelve_am_is_midnight_and_twelve_pm_is_noon() {
        let p = parsed(
            include_str!("../../tests/fixtures/noon_midnight.txt"),
            now_sep_2026(),
        );
        // Sep 16 2026 00:00 PDT == Sep 16 07:00 UTC.
        assert_eq!(p.session.resets_at, Some(ms(2026, 9, 16, 7, 0)));
        // Sep 21 2026 12:00 PDT == Sep 21 19:00 UTC.
        assert_eq!(p.week_all.resets_at, Some(ms(2026, 9, 21, 19, 0)));
    }

    #[test]
    fn december_to_january_wraps_the_year() {
        let now = at(2026, 12, 28, 18, 0);
        let p = parsed(include_str!("../../tests/fixtures/dec_to_jan.txt"), now);
        // Dec 29 2026 05:00 PST (UTC-8) == Dec 29 13:00 UTC, same year.
        assert_eq!(p.session.resets_at, Some(ms(2026, 12, 29, 13, 0)));
        // Jan 3 08:00 PST would be 2026 under the naive rule, far more than
        // 30 days in the past, so it rolls to 2027: Jan 3 16:00 UTC.
        assert_eq!(p.week_all.resets_at, Some(ms(2027, 1, 3, 16, 0)));
    }

    #[test]
    fn dst_gap_resolves_to_the_first_valid_instant_after_the_gap() {
        let now = at(2026, 3, 7, 12, 0);
        let p = parsed(include_str!("../../tests/fixtures/dst_gap.txt"), now);
        // 2026-03-08 02:30 America/New_York does not exist; the first valid
        // local instant after the gap is 03:00 EDT == 07:00 UTC.
        assert_eq!(p.session.resets_at, Some(ms(2026, 3, 8, 7, 0)));
    }

    #[test]
    fn ambiguous_local_time_resolves_to_the_earliest() {
        let now = at(2026, 10, 31, 12, 0);
        let p = parsed(include_str!("../../tests/fixtures/dst_ambiguous.txt"), now);
        // 2026-11-01 01:30 America/New_York happens twice; the earliest is
        // EDT (UTC-4) == 05:30 UTC.
        assert_eq!(p.session.resets_at, Some(ms(2026, 11, 1, 5, 30)));
    }

    #[test]
    fn empty_text_is_no_usage_data() {
        assert_eq!(parse_usage("", now_sep_2026()), PollOutcome::NoUsageData);
    }
}
