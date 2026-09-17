use rusqlite::{params, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::warn;

use crate::error::{AppError, AppResult};
use crate::store::Store;
use crate::usage::{ModelWindow, OutcomeKind, PollOutcome, SnapshotDto, Window};

/// D10: 30 days, in milliseconds.
pub const RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;

// History query limits (spec §3.1). The first three are mirrored in
// src/lib/historyLimits.json for the UI; a test below pins them together.
pub const MIN_BUCKET_MS: i64 = 60_000;
pub const MAX_BUCKETS: i64 = 1_000;
/// 30 days; equals RETENTION_MS (asserted in tests). Digit literal on purpose.
pub const MAX_RANGE_MS: i64 = 2_592_000_000;
/// Grace for a client that computed `since` a little before the command ran.
pub const RANGE_SLACK_MS: i64 = 300_000;
/// Model labels longer than this are neither offered nor accepted.
pub const MAX_LABEL_LEN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HistoryPoint {
    pub t: i64,
    pub pct: u8,
}

/// Which stored value a history query buckets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryMetric {
    WeekAll,
    Session,
    Model { label: String },
}

const DTO_COLS: &str = "id, account_id, taken_at, outcome, session_pct, session_resets_at, \
                        week_all_pct, week_all_resets_at, week_models, error, duration_ms";

fn window_from_cols(pct: Option<i64>, resets_at: Option<i64>) -> Option<Window> {
    pct.map(|p| Window {
        pct: p.clamp(0, 100) as u8,
        resets_at,
    })
}

fn row_to_dto(row: &Row<'_>) -> Result<SnapshotDto, rusqlite::Error> {
    let outcome_raw: String = row.get("outcome")?;
    let outcome = OutcomeKind::from_wire(&outcome_raw)
        .unwrap_or(OutcomeKind::ParseError)
        .as_str();
    let models_json: Option<String> = row.get("week_models")?;
    let week_models: Vec<ModelWindow> = models_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<Vec<ModelWindow>>(j).ok())
        .unwrap_or_default();

    Ok(SnapshotDto {
        id: row.get("id")?,
        account_id: row.get("account_id")?,
        taken_at: row.get("taken_at")?,
        outcome,
        session: window_from_cols(row.get("session_pct")?, row.get("session_resets_at")?),
        week_all: window_from_cols(row.get("week_all_pct")?, row.get("week_all_resets_at")?),
        week_models,
        error: row.get("error")?,
        duration_ms: row.get::<_, i64>("duration_ms")?.clamp(0, i64::from(u32::MAX)) as u32,
    })
}

impl Store {
    /// D8: raw text is stored with every snapshot, success or failure.
    pub fn insert_snapshot(
        &self,
        account_id: &str,
        taken_at: i64,
        outcome: &PollOutcome,
        raw: Option<&str>,
        duration_ms: u32,
    ) -> AppResult<i64> {
        let (session, week_all, models_json) = match outcome {
            PollOutcome::Ok(p) => {
                let models: Vec<ModelWindow> = p
                    .week_models
                    .iter()
                    .map(|(label, w)| ModelWindow {
                        label: label.clone(),
                        pct: w.pct,
                        resets_at: w.resets_at,
                    })
                    .collect();
                (
                    Some(p.session),
                    Some(p.week_all),
                    Some(serde_json::to_string(&models)?),
                )
            }
            _ => (None, None, None),
        };

        self.with_conn(|c| {
            c.execute(
                "INSERT INTO snapshots(account_id, taken_at, outcome, session_pct,
                     session_resets_at, week_all_pct, week_all_resets_at, week_models,
                     error, raw, duration_ms)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    account_id,
                    taken_at,
                    outcome.kind().as_str(),
                    session.map(|w| i64::from(w.pct)),
                    session.and_then(|w| w.resets_at),
                    week_all.map(|w| i64::from(w.pct)),
                    week_all.and_then(|w| w.resets_at),
                    models_json,
                    outcome.error_text(),
                    raw,
                    i64::from(duration_ms),
                ],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    /// Newest snapshot per account: max `taken_at`, tiebreak max `id`.
    pub fn latest_per_account(&self) -> AppResult<HashMap<String, SnapshotDto>> {
        self.with_conn(|c| {
            let sql = format!(
                "SELECT {DTO_COLS} FROM snapshots s
                 WHERE s.id = (SELECT x.id FROM snapshots x
                               WHERE x.account_id = s.account_id
                               ORDER BY x.taken_at DESC, x.id DESC LIMIT 1)"
            );
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt
                .query_map([], row_to_dto)?
                .collect::<Result<Vec<SnapshotDto>, rusqlite::Error>>()?;
            Ok(rows
                .into_iter()
                .map(|d| (d.account_id.clone(), d))
                .collect())
        })
    }

    /// Buckets of `MAX(metric)` over `ok` rows, anchored at `since`
    /// (`bucket = since + floor((taken_at - since) / bucket_ms) * bucket_ms`),
    /// only for buckets that have at least one row. No zero-filling: the
    /// process gate guarantees overnight gaps and those must render as
    /// breaks. Limits are enforced by the caller (`commands::core_get_history`).
    pub fn history(
        &self,
        account_id: &str,
        since: i64,
        bucket_ms: i64,
        metric: &HistoryMetric,
    ) -> AppResult<Vec<HistoryPoint>> {
        const SCALAR: &str = "SELECT ?2 + ((taken_at - ?2) / ?3) * ?3 AS bucket, MAX({col}) AS pct
             FROM snapshots
             WHERE account_id = ?1 AND taken_at >= ?2
               AND outcome = 'ok' AND {col} IS NOT NULL
             GROUP BY bucket
             ORDER BY bucket ASC";
        // `json_each` raises on malformed input BEFORE any WHERE term can
        // filter the row, so the guard lives in its argument. Element
        // fields are read from the root document via `m.fullkey` so a
        // scalar element ("abc" instead of {…}) yields NULL, never an error.
        const MODEL: &str = "SELECT ?2 + ((s.taken_at - ?2) / ?3) * ?3 AS bucket,
                    MAX(json_extract(s.week_models, m.fullkey || '.pct')) AS pct
             FROM snapshots AS s,
                  json_each(CASE WHEN json_valid(s.week_models) AND json_type(s.week_models) = 'array'
                                 THEN s.week_models ELSE '[]' END) AS m
             WHERE s.account_id = ?1 AND s.taken_at >= ?2
               AND s.outcome = 'ok' AND s.week_models IS NOT NULL
               AND json_extract(s.week_models, m.fullkey || '.label') = ?4
               AND json_type(s.week_models, m.fullkey || '.pct') IN ('integer', 'real')
             GROUP BY bucket
             ORDER BY bucket ASC";

        let (sql, label): (String, Option<&str>) = match metric {
            HistoryMetric::WeekAll => (SCALAR.replace("{col}", "week_all_pct"), None),
            HistoryMetric::Session => (SCALAR.replace("{col}", "session_pct"), None),
            HistoryMetric::Model { label } => (MODEL.to_string(), Some(label.as_str())),
        };

        self.with_conn(|c| {
            let mut stmt = c.prepare(&sql)?;
            let map = |r: &Row<'_>| -> Result<HistoryPoint, rusqlite::Error> {
                // JSON numbers may come back as REAL; integers widen losslessly.
                let pct: f64 = r.get("pct")?;
                Ok(HistoryPoint {
                    t: r.get::<_, i64>("bucket")?,
                    pct: pct.round().clamp(0.0, 100.0) as u8,
                })
            };
            let rows = match label {
                Some(l) => stmt
                    .query_map(params![account_id, since, bucket_ms, l], map)?
                    .collect::<Result<Vec<HistoryPoint>, rusqlite::Error>>()?,
                None => stmt
                    .query_map(params![account_id, since, bucket_ms], map)?
                    .collect::<Result<Vec<HistoryPoint>, rusqlite::Error>>()?,
            };
            Ok(rows)
        })
    }

    /// Distinct model labels seen in `ok` rows since `since`, sorted. Only
    /// labels the metric validation would accept (non-blank after trimming
    /// space/tab/LF/CR, at most `MAX_LABEL_LEN` characters) are returned, so
    /// the picker can never offer something `history` would reject.
    pub fn history_models(&self, account_id: &str, since: i64) -> AppResult<Vec<String>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT DISTINCT json_extract(s.week_models, m.fullkey || '.label') AS label
                 FROM snapshots AS s,
                      json_each(CASE WHEN json_valid(s.week_models) AND json_type(s.week_models) = 'array'
                                     THEN s.week_models ELSE '[]' END) AS m
                 WHERE s.account_id = ?1 AND s.taken_at >= ?2
                   AND s.outcome = 'ok' AND s.week_models IS NOT NULL
                   AND typeof(label) = 'text'
                   AND length(trim(label, char(32, 9, 10, 13))) >= 1
                   AND length(label) <= ?3
                 ORDER BY label ASC",
            )?;
            let rows = stmt
                .query_map(params![account_id, since, MAX_LABEL_LEN as i64], |r| r.get::<_, String>("label"))?
                .collect::<Result<Vec<String>, rusqlite::Error>>()?;
            Ok(rows)
        })
    }

    /// D10: drop rows older than the retention window, then reclaim pages.
    pub fn prune(&self, now: i64) -> AppResult<usize> {
        let cutoff = now - RETENTION_MS;
        let removed = self.with_conn(|c| {
            Ok(c.execute(
                "DELETE FROM snapshots WHERE taken_at < ?1",
                params![cutoff],
            )?)
        })?;
        if removed > 0 {
            warn!(removed, cutoff, "pruned snapshots older than the retention window");
        }
        self.with_conn(|c| {
            c.execute_batch("PRAGMA incremental_vacuum;")?;
            Ok(())
        })?;
        Ok(removed)
    }

    /// Backing store for `get_snapshot_raw`: `(raw, error)`.
    pub fn snapshot_raw(&self, snapshot_id: i64) -> AppResult<(Option<String>, Option<String>)> {
        self.with_conn(|c| {
            let mut stmt =
                c.prepare("SELECT raw, error FROM snapshots WHERE id = ?1")?;
            let mut rows = stmt.query(params![snapshot_id])?;
            match rows.next()? {
                Some(row) => Ok((row.get(0)?, row.get(1)?)),
                None => Err(AppError::NotFound(format!(
                    "no such snapshot: {snapshot_id}"
                ))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::usage::{Parsed, Window};

    const NOW: i64 = 1_700_000_000_000;
    const HOUR: i64 = 3_600_000;

    fn store_with_account() -> (tempfile::TempDir, Store, String) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let dir = tmp.path().join(".claude3");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let a = store
            .add_account(&dir, true, None, NOW)
            .expect("add account");
        (tmp, store, a.id)
    }

    fn ok_outcome(session: u8, week: u8) -> PollOutcome {
        PollOutcome::Ok(Parsed {
            session: Window { pct: session, resets_at: Some(NOW + HOUR) },
            week_all: Window { pct: week, resets_at: Some(NOW + 6 * HOUR) },
            week_models: vec![(
                "Fable".to_string(),
                Window { pct: 5, resets_at: Some(NOW + 6 * HOUR) },
            )],
        })
    }

    fn ok_with_models(session: u8, week: u8, models: &[(&str, u8)]) -> PollOutcome {
        PollOutcome::Ok(Parsed {
            session: Window { pct: session, resets_at: None },
            week_all: Window { pct: week, resets_at: None },
            week_models: models
                .iter()
                .map(|(l, p)| ((*l).to_string(), Window { pct: *p, resets_at: None }))
                .collect(),
        })
    }

    #[test]
    fn limits_match_the_shared_json_the_ui_reads() {
        let raw = include_str!("../../../src/lib/historyLimits.json");
        let v: serde_json::Value = serde_json::from_str(raw).expect("valid json");
        assert_eq!(v["minBucketMs"], MIN_BUCKET_MS);
        assert_eq!(v["maxBuckets"], MAX_BUCKETS);
        assert_eq!(v["maxRangeMs"], MAX_RANGE_MS);
        assert_eq!(MAX_RANGE_MS, RETENTION_MS, "the chart can reach exactly as far as retention");
    }

    #[test]
    fn history_metric_deserialises_from_the_tagged_wire_shape() {
        let w: HistoryMetric = serde_json::from_str(r#"{"kind":"week_all"}"#).expect("week_all");
        let s: HistoryMetric = serde_json::from_str(r#"{"kind":"session"}"#).expect("session");
        let m: HistoryMetric =
            serde_json::from_str(r#"{"kind":"model","label":"Fable"}"#).expect("model");
        assert_eq!(w, HistoryMetric::WeekAll);
        assert_eq!(s, HistoryMetric::Session);
        assert_eq!(m, HistoryMetric::Model { label: "Fable".into() });
        assert!(serde_json::from_str::<HistoryMetric>(r#"{"kind":"nope"}"#).is_err());
    }

    #[test]
    fn history_buckets_are_anchored_at_since_and_take_the_maximum() {
        let (_tmp, store, acct) = store_with_account();
        // `since` deliberately NOT on an hour boundary: buckets start at since.
        let since = NOW + 12_345;
        let bucket = 15 * 60_000; // 15 minutes
        store.insert_snapshot(&acct, since + 1_000, &ok_outcome(1, 4), None, 1).expect("a");
        store.insert_snapshot(&acct, since + 2_000, &ok_outcome(1, 9), None, 1).expect("b");
        store.insert_snapshot(&acct, since + bucket + 1, &ok_outcome(1, 6), None, 1).expect("c");
        store.insert_snapshot(&acct, since - 1, &ok_outcome(1, 99), None, 1).expect("before since");

        let points = store
            .history(&acct, since, bucket, &HistoryMetric::WeekAll)
            .expect("history");
        assert_eq!(points, vec![
            HistoryPoint { t: since, pct: 9 },
            HistoryPoint { t: since + bucket, pct: 6 },
        ]);
    }

    #[test]
    fn history_leaves_empty_buckets_out_entirely() {
        let (_tmp, store, acct) = store_with_account();
        let since = NOW;
        let bucket = 3_600_000;
        store.insert_snapshot(&acct, since + 1000, &ok_outcome(1, 4), None, 1).expect("a");
        // Skip three hours entirely, as the process gate does overnight.
        store.insert_snapshot(&acct, since + 4 * bucket + 1000, &ok_outcome(1, 7), None, 1).expect("b");

        let points = store.history(&acct, since, bucket, &HistoryMetric::WeekAll).expect("history");
        assert_eq!(points.len(), 2, "no zero-filling of the gap");
        assert_eq!(points[0].t, since);
        assert_eq!(points[1].t, since + 4 * bucket);
    }

    #[test]
    fn history_ignores_non_ok_rows_for_every_metric() {
        let (_tmp, store, acct) = store_with_account();
        store.insert_snapshot(&acct, NOW + 1000, &PollOutcome::Timeout(30), None, 1).expect("a");
        store.insert_snapshot(&acct, NOW + 2000, &PollOutcome::NoUsageData, None, 1).expect("b");
        for metric in [
            HistoryMetric::WeekAll,
            HistoryMetric::Session,
            HistoryMetric::Model { label: "Fable".into() },
        ] {
            assert!(store.history(&acct, NOW, 60_000, &metric).expect("history").is_empty());
        }
    }

    #[test]
    fn history_session_metric_reads_session_pct() {
        let (_tmp, store, acct) = store_with_account();
        store.insert_snapshot(&acct, NOW + 1000, &ok_outcome(37, 4), None, 1).expect("a");
        store.insert_snapshot(&acct, NOW + 2000, &ok_outcome(52, 9), None, 1).expect("b");
        let points = store.history(&acct, NOW, 60_000, &HistoryMetric::Session).expect("history");
        assert_eq!(points, vec![HistoryPoint { t: NOW, pct: 52 }]);
    }

    #[test]
    fn history_model_metric_matches_the_label_exactly_and_ignores_other_models() {
        let (_tmp, store, acct) = store_with_account();
        store
            .insert_snapshot(&acct, NOW + 1000, &ok_with_models(1, 1, &[("Fable", 40), ("Opus", 90)]), None, 1)
            .expect("a");
        store
            .insert_snapshot(&acct, NOW + 2000, &ok_with_models(1, 1, &[("Fable", 45)]), None, 1)
            .expect("b");
        store
            .insert_snapshot(&acct, NOW + 3000, &ok_with_models(1, 1, &[("Opus", 95)]), None, 1)
            .expect("c: no Fable at all");

        let fable = store
            .history(&acct, NOW, 60_000, &HistoryMetric::Model { label: "Fable".into() })
            .expect("fable");
        assert_eq!(fable, vec![HistoryPoint { t: NOW, pct: 45 }]);

        let lower = store
            .history(&acct, NOW, 60_000, &HistoryMetric::Model { label: "fable".into() })
            .expect("case differs");
        assert!(lower.is_empty(), "label match is exact");
    }

    #[test]
    fn history_model_metric_tolerates_a_malformed_json_row_and_a_real_pct() {
        let (_tmp, store, acct) = store_with_account();
        store
            .insert_snapshot(&acct, NOW + 1000, &ok_with_models(1, 1, &[("Fable", 40)]), None, 1)
            .expect("good row");
        store
            .with_conn(|c| {
                c.execute(
                    "INSERT INTO snapshots(account_id, taken_at, outcome, week_models, duration_ms)
                     VALUES(?1, ?2, 'ok', '{not json', 1)",
                    params![acct, NOW + 2000],
                )?;
                c.execute(
                    "INSERT INTO snapshots(account_id, taken_at, outcome, week_models, duration_ms)
                     VALUES(?1, ?2, 'ok', '[{\"label\":\"Fable\",\"pct\":47.6,\"resets_at\":null}]', 1)",
                    params![acct, NOW + 3000],
                )?;
                // Valid JSON but not an array, and an array with a scalar
                // element plus a text pct: all must be ignored, never raise.
                c.execute(
                    "INSERT INTO snapshots(account_id, taken_at, outcome, week_models, duration_ms)
                     VALUES(?1, ?2, 'ok', '\"abc\"', 1)",
                    params![acct, NOW + 4000],
                )?;
                c.execute(
                    "INSERT INTO snapshots(account_id, taken_at, outcome, week_models, duration_ms)
                     VALUES(?1, ?2, 'ok', '[5, {\"label\":\"Fable\",\"pct\":\"99\"}]', 1)",
                    params![acct, NOW + 5000],
                )?;
                Ok(())
            })
            .expect("raw inserts");

        let points = store
            .history(&acct, NOW, 60_000, &HistoryMetric::Model { label: "Fable".into() })
            .expect("malformed and odd-shaped rows must not fail the query");
        assert_eq!(points, vec![HistoryPoint { t: NOW, pct: 48 }], "47.6 rounds to 48; text pct ignored");
        let labels = store.history_models(&acct, NOW).expect("labels must not fail either");
        assert_eq!(labels, vec!["Fable".to_string()]);
    }

    #[test]
    fn history_models_lists_distinct_sorted_labels_within_the_window() {
        let (_tmp, store, acct) = store_with_account();
        store
            .insert_snapshot(&acct, NOW + 1000, &ok_with_models(1, 1, &[("Opus", 1), ("Fable", 2)]), None, 1)
            .expect("a");
        store
            .insert_snapshot(&acct, NOW + 2000, &ok_with_models(1, 1, &[("Fable", 3)]), None, 1)
            .expect("b");
        store
            .insert_snapshot(&acct, NOW - 10, &ok_with_models(1, 1, &[("Ancient", 3)]), None, 1)
            .expect("before since");
        store
            .insert_snapshot(&acct, NOW + 3000, &PollOutcome::Timeout(30), None, 1)
            .expect("failure row has no models");
        let long = "x".repeat(65);
        store
            .insert_snapshot(&acct, NOW + 4000, &ok_with_models(1, 1, &[(long.as_str(), 1), (" \t ", 1)]), None, 1)
            .expect("unusable labels");

        let labels = store.history_models(&acct, NOW).expect("labels");
        assert_eq!(labels, vec!["Fable".to_string(), "Opus".to_string()]);
    }

    #[test]
    fn an_ok_snapshot_round_trips_into_a_dto() {
        let (_tmp, store, acct) = store_with_account();
        let id = store
            .insert_snapshot(&acct, NOW, &ok_outcome(15, 4), Some("raw text"), 3012)
            .expect("insert");
        assert!(id > 0);

        let latest = store.latest_per_account().expect("latest");
        let dto = latest.get(&acct).expect("row for account");
        assert_eq!(dto.outcome, "ok");
        assert_eq!(dto.session, Some(Window { pct: 15, resets_at: Some(NOW + HOUR) }));
        assert_eq!(
            dto.week_all,
            Some(Window { pct: 4, resets_at: Some(NOW + 6 * HOUR) })
        );
        assert_eq!(dto.week_models.len(), 1);
        assert_eq!(dto.week_models[0].label, "Fable");
        assert_eq!(dto.week_models[0].pct, 5);
        assert_eq!(dto.error, None);
        assert_eq!(dto.duration_ms, 3012);
    }

    #[test]
    fn a_failure_snapshot_stores_its_message_and_no_windows() {
        let (_tmp, store, acct) = store_with_account();
        store
            .insert_snapshot(
                &acct,
                NOW,
                &PollOutcome::SpawnError("exit 1: boom".into()),
                Some("stderr tail"),
                412,
            )
            .expect("insert");

        let latest = store.latest_per_account().expect("latest");
        let dto = latest.get(&acct).expect("row");
        assert_eq!(dto.outcome, "spawn_error");
        assert_eq!(dto.session, None);
        assert_eq!(dto.week_all, None);
        assert!(dto.week_models.is_empty());
        assert_eq!(dto.error.as_deref(), Some("exit 1: boom"));
    }

    #[test]
    fn raw_is_stored_for_every_outcome() {
        let (_tmp, store, acct) = store_with_account();
        let ok_id = store
            .insert_snapshot(&acct, NOW, &ok_outcome(1, 1), Some("ok raw"), 10)
            .expect("insert ok");
        let fail_id = store
            .insert_snapshot(
                &acct,
                NOW + 1,
                &PollOutcome::Timeout(30),
                Some("timeout raw"),
                30_000,
            )
            .expect("insert fail");

        assert_eq!(
            store.snapshot_raw(ok_id).expect("raw"),
            (Some("ok raw".to_string()), None)
        );
        assert_eq!(
            store.snapshot_raw(fail_id).expect("raw"),
            (
                Some("timeout raw".to_string()),
                Some("timed out after 30s".to_string())
            )
        );
    }

    #[test]
    fn snapshot_raw_returns_the_error_text_too() {
        let (_tmp, store, acct) = store_with_account();
        let id = store
            .insert_snapshot(
                &acct,
                NOW,
                &PollOutcome::ParseError("missing session line".into()),
                Some("report text"),
                900,
            )
            .expect("insert");
        assert_eq!(
            store.snapshot_raw(id).expect("raw"),
            (
                Some("report text".to_string()),
                Some("missing session line".to_string())
            )
        );
    }

    #[test]
    fn snapshot_raw_for_an_unknown_id_is_not_found() {
        let (_tmp, store, _acct) = store_with_account();
        let err = store.snapshot_raw(9999).expect_err("must reject");
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn latest_per_account_breaks_a_same_millisecond_tie_on_the_highest_id() {
        let (_tmp, store, acct) = store_with_account();
        store
            .insert_snapshot(&acct, NOW, &ok_outcome(10, 10), None, 1)
            .expect("first");
        store
            .insert_snapshot(&acct, NOW, &ok_outcome(20, 20), None, 1)
            .expect("second");

        let latest = store.latest_per_account().expect("latest");
        let dto = latest.get(&acct).expect("row");
        assert_eq!(dto.session.map(|w| w.pct), Some(20));
    }

    #[test]
    fn prune_deletes_rows_strictly_older_than_the_retention_window() {
        let (_tmp, store, acct) = store_with_account();
        let boundary = NOW - RETENTION_MS;
        store
            .insert_snapshot(&acct, boundary - 1, &ok_outcome(1, 1), None, 1)
            .expect("older");
        store
            .insert_snapshot(&acct, boundary, &ok_outcome(2, 2), None, 1)
            .expect("exactly at the boundary");
        store
            .insert_snapshot(&acct, boundary + 1, &ok_outcome(3, 3), None, 1)
            .expect("newer");

        let removed = store.prune(NOW).expect("prune");
        assert_eq!(removed, 1);

        let remaining: i64 = store
            .with_conn(|c| Ok(c.query_row("SELECT COUNT(*) FROM snapshots", [], |r| r.get(0))?))
            .expect("count");
        assert_eq!(remaining, 2);
    }

    #[test]
    fn prune_on_an_empty_table_removes_nothing() {
        let (_tmp, store, _acct) = store_with_account();
        assert_eq!(store.prune(NOW).expect("prune"), 0);
    }

    #[test]
    fn removing_an_account_cascades_its_snapshots() {
        let (_tmp, store, acct) = store_with_account();
        store
            .insert_snapshot(&acct, NOW, &ok_outcome(1, 1), Some("raw"), 1)
            .expect("insert");
        store.remove_account(&acct).expect("remove");

        let remaining: i64 = store
            .with_conn(|c| Ok(c.query_row("SELECT COUNT(*) FROM snapshots", [], |r| r.get(0))?))
            .expect("count");
        assert_eq!(remaining, 0, "foreign_keys=ON must cascade the delete");
    }

    #[test]
    fn latest_per_account_covers_several_accounts_independently() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open_in_memory().expect("open");
        let mut ids = Vec::new();
        for name in [".claude", ".claude3"] {
            let d = tmp.path().join(name);
            std::fs::create_dir_all(&d).expect("mkdir");
            ids.push(store.add_account(&d, true, None, NOW).expect("add").id);
        }
        store
            .insert_snapshot(&ids[0], NOW, &ok_outcome(11, 11), None, 1)
            .expect("a");
        store
            .insert_snapshot(&ids[1], NOW, &ok_outcome(22, 22), None, 1)
            .expect("b");

        let latest = store.latest_per_account().expect("latest");
        assert_eq!(latest.len(), 2);
        assert_eq!(latest[&ids[0]].session.map(|w| w.pct), Some(11));
        assert_eq!(latest[&ids[1]].session.map(|w| w.pct), Some(22));
    }
}
