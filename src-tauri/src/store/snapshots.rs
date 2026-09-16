use rusqlite::{params, Row};
use serde::Serialize;
use std::collections::HashMap;
use tracing::warn;

use crate::error::{AppError, AppResult};
use crate::store::Store;
use crate::usage::{ModelWindow, OutcomeKind, PollOutcome, SnapshotDto, Window};

/// D10: 30 days, in milliseconds.
pub const RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;

const HOUR_MS: i64 = 3_600_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HistoryPoint {
    pub t: i64,
    pub pct: u8,
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

    /// Hourly buckets of `max(week_all_pct)` over `ok` rows, only for hours
    /// that have at least one row. No zero-filling: the process gate
    /// guarantees overnight gaps and those must render as breaks.
    pub fn history(&self, account_id: &str, since: i64) -> AppResult<Vec<HistoryPoint>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT (taken_at / ?3) * ?3 AS bucket, MAX(week_all_pct) AS pct
                 FROM snapshots
                 WHERE account_id = ?1 AND taken_at >= ?2
                   AND outcome = 'ok' AND week_all_pct IS NOT NULL
                 GROUP BY bucket
                 ORDER BY bucket ASC",
            )?;
            let rows = stmt
                .query_map(params![account_id, since, HOUR_MS], |r| {
                    Ok(HistoryPoint {
                        t: r.get::<_, i64>("bucket")?,
                        pct: r.get::<_, i64>("pct")?.clamp(0, 100) as u8,
                    })
                })?
                .collect::<Result<Vec<HistoryPoint>, rusqlite::Error>>()?;
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
            .add_account(&dir, true, None, true, NOW)
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
    fn history_buckets_by_hour_and_takes_the_maximum_week_all_pct() {
        let (_tmp, store, acct) = store_with_account();
        let h0 = 1_700_000_000_000 - (1_700_000_000_000 % HOUR);
        store
            .insert_snapshot(&acct, h0 + 60_000, &ok_outcome(1, 4), None, 1)
            .expect("a");
        store
            .insert_snapshot(&acct, h0 + 120_000, &ok_outcome(1, 9), None, 1)
            .expect("b");
        store
            .insert_snapshot(&acct, h0 + HOUR + 60_000, &ok_outcome(1, 6), None, 1)
            .expect("c");

        let points = store.history(&acct, h0 - HOUR).expect("history");
        assert_eq!(points.len(), 2);
        assert_eq!(points[0], HistoryPoint { t: h0, pct: 9 });
        assert_eq!(points[1], HistoryPoint { t: h0 + HOUR, pct: 6 });
    }

    #[test]
    fn history_leaves_empty_hours_out_entirely() {
        let (_tmp, store, acct) = store_with_account();
        let h0 = 1_700_000_000_000 - (1_700_000_000_000 % HOUR);
        store
            .insert_snapshot(&acct, h0 + 1000, &ok_outcome(1, 4), None, 1)
            .expect("a");
        // Skip three hours entirely, as the process gate does overnight.
        store
            .insert_snapshot(&acct, h0 + 4 * HOUR + 1000, &ok_outcome(1, 7), None, 1)
            .expect("b");

        let points = store.history(&acct, h0 - HOUR).expect("history");
        assert_eq!(points.len(), 2, "no zero-filling of the gap");
        assert_eq!(points[0].t, h0);
        assert_eq!(points[1].t, h0 + 4 * HOUR);
    }

    #[test]
    fn history_ignores_non_ok_rows() {
        let (_tmp, store, acct) = store_with_account();
        let h0 = 1_700_000_000_000 - (1_700_000_000_000 % HOUR);
        store
            .insert_snapshot(&acct, h0 + 1000, &PollOutcome::Timeout(30), None, 1)
            .expect("a");
        store
            .insert_snapshot(&acct, h0 + 2000, &PollOutcome::NoUsageData, None, 1)
            .expect("b");
        assert!(store.history(&acct, h0 - HOUR).expect("history").is_empty());
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
            ids.push(store.add_account(&d, true, None, false, NOW).expect("add").id);
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
