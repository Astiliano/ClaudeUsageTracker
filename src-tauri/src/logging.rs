use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{reload, EnvFilter, Registry};

use crate::error::{AppError, AppResult};

pub const LOG_FILE_PREFIX: &str = "claude-usage-tracker";
pub const LOG_FILES_KEPT: usize = 7;

/// D13: only two levels are user-selectable. `info` keeps milestones;
/// `debug` is the firehose, one switch away.
pub fn filter_for(level: &str) -> AppResult<EnvFilter> {
    let directive = match level {
        "info" => "info",
        "debug" => "debug",
        other => {
            return Err(AppError::OutOfRange(format!(
                "log_level must be info or debug, got {other}"
            )))
        }
    };
    EnvFilter::try_new(directive)
        .map_err(|e| AppError::Internal(format!("invalid log filter: {e}")))
}

/// Keeps the reload handle and the non-blocking writer's worker alive.
/// Dropping it flushes and stops the writer thread.
#[derive(Debug)]
pub struct LogHandle {
    reload: reload::Handle<EnvFilter, Registry>,
    _guard: WorkerGuard,
}

impl LogHandle {
    /// Applied immediately by `set_settings`.
    pub fn set_level(&self, level: &str) -> AppResult<()> {
        let filter = filter_for(level)?;
        self.reload
            .reload(filter)
            .map_err(|e| AppError::Internal(format!("could not reload log filter: {e}")))?;
        tracing::info!(level, "log level changed");
        Ok(())
    }
}

/// JSON lines to a daily-rotating file in the app log dir, with the level
/// behind a reload handle. Returns an `internal` error if a global subscriber
/// is already installed in this process.
pub fn init_logging(log_dir: &Path, level: &str) -> AppResult<LogHandle> {
    crate::paths::ensure_dir(log_dir)?;

    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix("log")
        .max_log_files(LOG_FILES_KEPT)
        .build(log_dir)
        .map_err(|e| AppError::Io(format!("could not open log file: {e}")))?;

    let (writer, guard) = tracing_appender::non_blocking(appender);

    let (filter_layer, reload_handle) = reload::Layer::new(filter_for(level)?);
    let fmt_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_current_span(false)
        .with_span_list(false)
        .with_target(true)
        .with_writer(writer);

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .try_init()
        .map_err(|e| AppError::Internal(format!("logging already initialised: {e}")))?;

    tracing::info!(
        log_dir = %log_dir.display(),
        level,
        files_kept = LOG_FILES_KEPT,
        "logging initialised"
    );

    Ok(LogHandle {
        reload: reload_handle,
        _guard: guard,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // `init_logging` installs a process-global tracing subscriber, which
    // `tracing_subscriber::registry().try_init()` allows exactly once per
    // process (not once per test). Under plain `cargo test` the test
    // harness runs this file's tests concurrently on multiple threads, so
    // two tests both calling `init_logging` would race on that global
    // one-shot resource with no guaranteed winner. Serialising with this
    // lock (rather than relying on `--test-threads=1`) makes ordering
    // deterministic within the file; the single test below that performs a
    // real init is written to also cover the "second init in this process
    // fails" case itself, so no cross-test ordering assumption remains.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn info_and_debug_are_the_only_accepted_levels() {
        let _guard = lock();
        assert!(filter_for("info").is_ok());
        assert!(filter_for("debug").is_ok());
    }

    #[test]
    fn an_unknown_level_is_out_of_range() {
        let _guard = lock();
        for bad in ["trace", "TRACE", "warn", "", "verbose"] {
            let err = filter_for(bad).expect_err("must reject");
            assert_eq!(err.code(), "out_of_range", "level {bad}");
        }
    }

    #[test]
    fn the_rotation_policy_matches_the_spec() {
        let _guard = lock();
        assert_eq!(LOG_FILES_KEPT, 7);
        assert_eq!(LOG_FILE_PREFIX, "claude-usage-tracker");
    }

    #[test]
    fn init_logging_writes_json_and_the_reload_handle_switches_levels() {
        let _guard = lock();
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("logs");
        let handle = init_logging(&dir, "info").expect("init");

        tracing::info!(probe = "hello", "startup probe");
        handle.set_level("debug").expect("raise level");
        tracing::debug!(probe = "hello", "debug probe");

        // The reload handle keeps rejecting unknown levels after a real
        // subscriber is installed.
        let err = handle.set_level("trace").expect_err("must reject");
        assert_eq!(err.code(), "out_of_range");

        // A global subscriber can only be installed once per process; a
        // second `init_logging` call in this same process must fail rather
        // than silently replacing the first one. This is exercised here,
        // in the same test as the successful init, so the outcome never
        // depends on which test the harness happens to run first.
        let second = init_logging(&tmp.path().join("logs2"), "info");
        let err = second.expect_err("global subscriber is one-shot per process");
        assert_eq!(err.code(), "internal");

        // The non-blocking writer flushes on its own schedule; give it a
        // moment, then assert a file exists with our prefix and that its
        // content is a well-formed JSON line carrying our fields.
        std::thread::sleep(std::time::Duration::from_millis(500));
        drop(handle);

        assert!(dir.is_dir(), "log dir must be created");
        let files: Vec<String> = std::fs::read_dir(&dir)
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        let log_file = files
            .iter()
            .find(|f| f.starts_with(LOG_FILE_PREFIX))
            .unwrap_or_else(|| panic!("expected a log file, found {files:?}"));

        let contents = std::fs::read_to_string(dir.join(log_file)).expect("read log file");
        let probe_line = contents
            .lines()
            .find(|line| line.contains("startup probe"))
            .expect("probe line was written");
        let value: serde_json::Value =
            serde_json::from_str(probe_line).expect("log line is a JSON object");
        assert_eq!(value["level"], "INFO");
        assert_eq!(value["fields"]["message"], "startup probe");
        assert_eq!(value["fields"]["probe"], "hello");
    }
}
