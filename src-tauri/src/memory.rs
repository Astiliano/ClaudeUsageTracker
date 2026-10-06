//! Memory figures behind one seam.
//!
//! Pure math (`floor_bytes`, `commit_headroom`, `merge_peak`, `recovery_due`,
//! `reading_log`) is split from the syscalls so it is unit-tested on every
//! platform. The driver's admission guard and the sampler's recovery wake
//! both read commit headroom through [`MemoryProbe`].

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Peak memory of one finished child process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ChildPeak {
    pub working_set_bytes: u64,
    pub commit_bytes: u64,
}

pub const MIB: u64 = 1_048_576;

/// The setting is in MiB; the guard compares bytes.
pub fn floor_bytes(min_free_memory_mb: u32) -> u64 {
    u64::from(min_free_memory_mb) * MIB
}

/// Free commit in bytes: `(limit - total)` pages times the page size,
/// saturating at 0 when the total exceeds the limit.
pub fn commit_headroom(limit_pages: u64, total_pages: u64, page_size: u64) -> u64 {
    limit_pages
        .saturating_sub(total_pages)
        .saturating_mul(page_size)
}

/// Free commit (CommitLimit - CommitTotal) in bytes, or `None` when the OS
/// call fails or the platform has no such figure.
#[cfg(windows)]
pub fn available_commit_bytes() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetPerformanceInfo, PERFORMANCE_INFORMATION,
    };

    // SAFETY: PERFORMANCE_INFORMATION is plain data of integers, for which
    // the all-zero bit pattern is a valid value.
    let mut pi: PERFORMANCE_INFORMATION = unsafe { std::mem::zeroed() };
    let cb = std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32;
    pi.cb = cb;
    // SAFETY: `pi` is a live, zeroed out-param and `cb` is its exact size.
    let ok = unsafe { K32GetPerformanceInfo(&mut pi, cb) };
    if ok == 0 {
        return None;
    }
    Some(commit_headroom(
        pi.CommitLimit as u64,
        pi.CommitTotal as u64,
        pi.PageSize as u64,
    ))
}

#[cfg(not(windows))]
pub fn available_commit_bytes() -> Option<u64> {
    None
}

/// Field-wise maximum; a present peak wins over an absent one.
pub fn merge_peak(a: Option<ChildPeak>, b: Option<ChildPeak>) -> Option<ChildPeak> {
    match (a, b) {
        (Some(a), Some(b)) => Some(ChildPeak {
            working_set_bytes: a.working_set_bytes.max(b.working_set_bytes),
            commit_bytes: a.commit_bytes.max(b.commit_bytes),
        }),
        (Some(p), None) | (None, Some(p)) => Some(p),
        (None, None) => None,
    }
}

/// True only while held and the reading is at or above the floor. A lost
/// reading never recovers a hold.
pub fn recovery_due(held: bool, available: Option<u64>, floor_bytes: u64) -> bool {
    held && available.is_some_and(|a| a >= floor_bytes)
}

/// A change in whether the reading is available.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadingLog {
    Lost,
    Restored,
}

/// Which transition, if any, this reading is. Steady states log nothing.
pub fn reading_log(was_lost: bool, reading: Option<u64>) -> Option<ReadingLog> {
    match (was_lost, reading) {
        (false, None) => Some(ReadingLog::Lost),
        (true, Some(_)) => Some(ReadingLog::Restored),
        _ => None,
    }
}

/// Peak working set and peak commit (pagefile usage) of a process.
#[cfg(windows)]
pub fn process_peak(handle: std::os::windows::io::RawHandle) -> Option<ChildPeak> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };

    // SAFETY: PROCESS_MEMORY_COUNTERS is plain data of integers, for which
    // the all-zero bit pattern is a valid value.
    let mut c: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    c.cb = cb;
    // A process handle is an opaque kernel id that this function never
    // dereferences: an invalid one makes the call fail with BOOL 0. It
    // travels as an integer so the safe signature does not trip
    // clippy::not_unsafe_ptr_arg_deref.
    let handle = handle as usize as HANDLE;
    // SAFETY: `c` is a live, zeroed out-param and `cb` is its exact size.
    // The handle is only looked up by the kernel; a stale or invalid value
    // returns 0 (mapped to None below), never memory unsafety.
    let ok = unsafe { K32GetProcessMemoryInfo(handle, &mut c, cb) };
    if ok == 0 {
        return None;
    }
    Some(ChildPeak {
        working_set_bytes: c.PeakWorkingSetSize as u64,
        commit_bytes: c.PeakPagefileUsage as u64,
    })
}

/// The one seam both readers use: the driver's guard and the sampler's
/// recovery wake. Tests inject a fake.
pub trait MemoryProbe: Send + Sync {
    fn available_commit_bytes(&self) -> Option<u64>;
}

pub struct RealMemoryProbe;

impl MemoryProbe for RealMemoryProbe {
    fn available_commit_bytes(&self) -> Option<u64> {
        available_commit_bytes()
    }
}

/// Every probe read in the driver and in the cycle task. The swap on the
/// shared flag yields `was_lost`, so two readers on different tasks log each
/// transition once between them.
pub fn read_memory(probe: &dyn MemoryProbe, lost: &AtomicBool) -> Option<u64> {
    let r = probe.available_commit_bytes();
    let was_lost = lost.swap(r.is_none(), Ordering::SeqCst);
    match reading_log(was_lost, r) {
        Some(ReadingLog::Lost) => tracing::warn!("memory reading unavailable; not holding"),
        Some(ReadingLog::Restored) => tracing::info!("memory reading restored"),
        None => {}
    }
    r
}

/// What an automatic cycle carries into its task.
#[derive(Clone)]
pub struct MemoryGuard {
    pub probe: Arc<dyn MemoryProbe>,
    pub floor_bytes: u64,
    pub lost: Arc<AtomicBool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    fn peak(ws: u64, commit: u64) -> ChildPeak {
        ChildPeak {
            working_set_bytes: ws,
            commit_bytes: commit,
        }
    }

    #[test]
    fn commit_headroom_multiplies_free_pages_by_page_size() {
        assert_eq!(commit_headroom(1000, 400, 4096), 600 * 4096);
    }

    #[test]
    fn commit_headroom_saturates_when_total_exceeds_limit() {
        assert_eq!(commit_headroom(400, 1000, 4096), 0);
    }

    #[test]
    fn floor_bytes_is_mebibytes() {
        assert_eq!(floor_bytes(1536), 1536 * 1_048_576);
        assert_eq!(floor_bytes(0), 0);
    }

    #[test]
    fn merge_peak_takes_each_field_maximum() {
        assert_eq!(
            merge_peak(Some(peak(5, 9)), Some(peak(7, 3))),
            Some(peak(7, 9))
        );
    }

    #[test]
    fn merge_peak_keeps_some_over_none() {
        let p = peak(5, 9);
        assert_eq!(merge_peak(Some(p), None), Some(p));
        assert_eq!(merge_peak(None, Some(p)), Some(p));
        assert_eq!(merge_peak(None, None), None);
    }

    #[test]
    fn recovery_due_only_while_held_and_at_or_above_floor() {
        let table: [(bool, Option<u64>, u64, bool); 5] = [
            (false, Some(10), 5, false),
            (true, Some(4), 5, false),
            (true, Some(5), 5, true),
            (true, Some(6), 5, true),
            (true, None, 5, false),
        ];
        for (held, available, floor, want) in table {
            assert_eq!(
                recovery_due(held, available, floor),
                want,
                "held={held} available={available:?} floor={floor}"
            );
        }
    }

    #[test]
    fn reading_log_fires_only_on_transitions() {
        assert_eq!(reading_log(false, None), Some(ReadingLog::Lost));
        assert_eq!(reading_log(true, Some(1)), Some(ReadingLog::Restored));
        assert_eq!(reading_log(false, Some(1)), None);
        assert_eq!(reading_log(true, None), None);
    }

    struct Scripted(Mutex<VecDeque<Option<u64>>>);

    impl MemoryProbe for Scripted {
        fn available_commit_bytes(&self) -> Option<u64> {
            self.0
                .lock()
                .expect("scripted probe lock")
                .pop_front()
                .expect("scripted probe ran out of readings")
        }
    }

    #[test]
    fn read_memory_sets_and_clears_the_shared_flag() {
        let probe = Scripted(Mutex::new(VecDeque::from([None, None, Some(1)])));
        let lost = Arc::new(AtomicBool::new(false));

        assert_eq!(read_memory(&probe, &lost), None);
        assert!(lost.load(Ordering::SeqCst));
        assert_eq!(read_memory(&probe, &lost), None);
        assert!(lost.load(Ordering::SeqCst));
        assert_eq!(read_memory(&probe, &lost), Some(1));
        assert!(!lost.load(Ordering::SeqCst));
    }

    #[cfg(windows)]
    #[test]
    fn available_commit_bytes_reads_a_positive_figure() {
        let start = std::time::Instant::now();
        let got = available_commit_bytes();
        let elapsed = start.elapsed();
        eprintln!("available_commit_bytes took {:?}", elapsed);
        let v = got.expect("K32GetPerformanceInfo should succeed");
        assert!(v > 0);
    }

    #[cfg(windows)]
    #[test]
    fn process_peak_of_this_process_is_positive() {
        use std::os::windows::io::RawHandle;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        // SAFETY: GetCurrentProcess takes no arguments and returns the
        // always-valid pseudo-handle for the calling process.
        let handle = unsafe { GetCurrentProcess() } as RawHandle;
        let p = process_peak(handle).expect("K32GetProcessMemoryInfo should succeed");
        assert!(p.working_set_bytes > 0 && p.commit_bytes > 0);
    }

    #[cfg(not(windows))]
    #[test]
    fn available_commit_bytes_is_none_off_windows() {
        assert_eq!(available_commit_bytes(), None);
    }
}
