//! The Windows side of the aspect lock: facts read straight from the HWND, the one apply
//! (`apply_hwnd`) shared by the `set_content_height` command and the subclass proc, and the
//! subclass proc itself, which holds the drag to the content's shape and resumes a pending fit.

use crate::error::{AppError, AppResult};
use crate::window_aspect::{
    client_bounds, fit_rect, run_guarded, step, AspectState, Edge, FitAction, FitPlan, Rect, Size,
    Step, WindowFacts,
};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowRect, IsIconic, IsZoomed, SetWindowPos, SIZE_RESTORED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOZORDER, WM_DPICHANGED, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_NCDESTROY,
    WM_SIZE, WM_SIZING,
};

const NO_RECT: RECT = RECT {
    left: 0,
    top: 0,
    right: 0,
    bottom: 0,
};

fn rect_of(r: RECT) -> Rect {
    Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

/// Reads the window's geometry with Win32 only. The proc must not use tauri getters, which may
/// re-enter tao's state lock from inside the proc. Any failed call gives `None`, and the caller
/// logs and leaves the fit pending.
///
/// # Safety
///
/// `hwnd` must be a live window created on the calling thread, so the calls below cannot race a
/// destroy. (Marked `unsafe` because a public function that passes a raw handle to FFI is a
/// `clippy::not_unsafe_ptr_arg_deref` error, and the proc and the tests are the only callers.)
pub unsafe fn hwnd_facts(hwnd: HWND) -> Option<WindowFacts> {
    let mut outer = NO_RECT;
    let mut client = NO_RECT;
    let mut info = MONITORINFO {
        // A few dozen bytes: the cast cannot truncate.
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        rcMonitor: NO_RECT,
        rcWork: NO_RECT,
        dwFlags: 0,
    };
    // SAFETY: `hwnd` is a window owned by this thread and the out-pointers are stack locals.
    unsafe {
        if GetWindowRect(hwnd, &mut outer) == 0 || GetClientRect(hwnd, &mut client) == 0 {
            return None;
        }
        let dpi = GetDpiForWindow(hwnd);
        if dpi == 0 {
            return None;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor.is_null() || GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        Some(WindowFacts {
            client: Size {
                w: client.right.saturating_sub(client.left),
                h: client.bottom.saturating_sub(client.top),
            },
            outer: rect_of(outer),
            work: Some(rect_of(info.rcWork)),
            dpi,
            maximized: IsZoomed(hwnd) != 0,
            minimized: IsIconic(hwnd) != 0,
        })
    }
}

/// The one apply: a single `SetWindowPos` carries position and size, adding `SWP_NOMOVE` when the
/// top is unchanged. It is synchronous (no `SWP_ASYNCWINDOWPOS`) because every caller is on the
/// window's thread. A failure logs a WARN and returns false so the caller re-marks the fit pending.
pub(crate) fn apply_hwnd(hwnd: HWND, plan: &FitPlan, label: &str) -> bool {
    let FitPlan::Resize { outer, .. } = plan else {
        return true;
    };
    let mut flags = SWP_NOZORDER | SWP_NOACTIVATE;
    let mut current = NO_RECT;
    // SAFETY: `hwnd` is a window owned by this thread; `current` is a stack local.
    if unsafe { GetWindowRect(hwnd, &mut current) } != 0 && current.top == outer.top {
        flags |= SWP_NOMOVE;
    }
    // SAFETY: as above; a null `hWndInsertAfter` is ignored under `SWP_NOZORDER`.
    let ok = unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            outer.left,
            outer.top,
            outer.right.saturating_sub(outer.left),
            outer.bottom.saturating_sub(outer.top),
            flags,
        )
    };
    if ok == 0 {
        // SAFETY: reads the calling thread's last-error value.
        let error = unsafe { GetLastError() };
        tracing::warn!(label, error, "SetWindowPos failed");
        return false;
    }
    true
}

/// Applies a fit to a Tauri window. Generic over the runtime so both the plugin's `Window<R>` and
/// the command's `Window` fit.
pub fn apply_fit<R: tauri::Runtime>(window: &tauri::Window<R>, plan: &FitPlan) -> AppResult<()> {
    if matches!(plan, FitPlan::Unchanged) {
        return Ok(());
    }
    let hwnd = window
        .hwnd()
        .map_err(|e| AppError::Internal(format!("window handle unavailable: {e}")))?;
    let raw: HWND = hwnd.0;
    if apply_hwnd(raw, plan, window.label()) {
        Ok(())
    } else {
        Err(AppError::Internal("SetWindowPos failed".into()))
    }
}

/// Identifies this subclass on a window. A window is subclassed once, by `install` from the
/// plugin's `on_window_ready`.
pub(crate) const SUBCLASS_ID: usize = 0x4355_5441;

/// Set after the first panic the proc contains, so the WARN is logged once per process.
static PANIC_WARNED: AtomicBool = AtomicBool::new(false);

/// The work the proc delegates, so tests can inject a failing apply or a panic after the forward.
/// Production code builds only `ProcHooks::REAL`.
pub(crate) struct ProcHooks {
    pub apply: fn(HWND, &FitPlan, &str) -> bool,
    pub after_forward: fn(u32),
}

impl ProcHooks {
    pub(crate) const REAL: ProcHooks = ProcHooks {
        apply: apply_hwnd,
        after_forward: |_| {},
    };
}

/// Owned by the subclass through its reference-data pointer: leaked in `install_hwnd_with`, freed
/// in `WM_NCDESTROY`.
struct SubclassData {
    state: Arc<AspectState>,
    label: String,
    hooks: ProcHooks,
}

pub(crate) fn install_hwnd(hwnd: HWND, state: Arc<AspectState>, label: String) -> Result<(), u32> {
    install_hwnd_with(hwnd, state, label, ProcHooks::REAL)
}

/// Subclasses `hwnd` (which must belong to the calling thread). On failure the data is freed and
/// the Win32 error code returned.
pub(crate) fn install_hwnd_with(
    hwnd: HWND,
    state: Arc<AspectState>,
    label: String,
    hooks: ProcHooks,
) -> Result<(), u32> {
    let data = Box::into_raw(Box::new(SubclassData {
        state,
        label,
        hooks,
    }));
    // SAFETY: `data` is a valid, uniquely owned pointer. On success the proc owns it until
    // `WM_NCDESTROY`; on failure nothing else holds it and it is freed below.
    let ok = unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, data as usize) };
    if ok == 0 {
        // SAFETY: reads the calling thread's last-error value.
        let code = unsafe { GetLastError() };
        // SAFETY: `data` came from `Box::into_raw` above and the failed call kept no copy.
        drop(unsafe { Box::from_raw(data) });
        return Err(code);
    }
    Ok(())
}

/// Subclasses a Tauri window. Returns whether the live aspect lock is active.
pub fn install<R: tauri::Runtime>(window: &tauri::Window<R>, state: Arc<AspectState>) -> bool {
    let label = window.label().to_string();
    let hwnd: HWND = match window.hwnd() {
        Ok(h) => h.0,
        Err(e) => {
            tracing::warn!(label, error = %e, "window handle unavailable");
            return false;
        }
    };
    match install_hwnd(hwnd, state, label.clone()) {
        Ok(()) => {
            // SAFETY: `hwnd` is the live window just subclassed.
            let dpi = unsafe { GetDpiForWindow(hwnd) };
            tracing::info!(label, dpi, "subclass installed");
            true
        }
        Err(code) => {
            tracing::warn!(label, error = code, "subclass install failed");
            false
        }
    }
}

/// What a resume attempt did, for the callers' log lines.
enum Resume {
    Applied(FitPlan),
    Unchanged,
    ApplyFailed,
    Skipped(FitAction),
    NoRatio,
    FactsUnavailable,
}

impl Resume {
    /// The `pending` field of the "size-move end" line.
    fn name(&self) -> &'static str {
        match self {
            Resume::Applied(_) => "applied",
            Resume::Unchanged => "unchanged",
            Resume::ApplyFailed => "apply_failed",
            Resume::Skipped(FitAction::SkipMaximized) => "skipped:maximized",
            Resume::Skipped(FitAction::SkipMinimized) => "skipped:minimized",
            Resume::Skipped(_) => "skipped:deferred",
            Resume::NoRatio => "skipped:no_ratio",
            Resume::FactsUnavailable => "facts_unavailable",
        }
    }
}

fn warn_facts_unavailable(label: &str) {
    // SAFETY: reads the calling thread's last-error value.
    let error = unsafe { GetLastError() };
    tracing::warn!(label, error, "window facts unavailable");
}

/// The one resume path of the proc: read the facts, decide, apply. A failed read or apply leaves
/// the fit pending.
fn resume(hwnd: HWND, d: &SubclassData) -> Resume {
    // SAFETY: `hwnd` is the window this proc is running for, on its own thread.
    let Some(f) = (unsafe { hwnd_facts(hwnd) }) else {
        warn_facts_unavailable(&d.label);
        return Resume::FactsUnavailable;
    };
    match step(&d.state, &f) {
        Step::NoRatio => Resume::NoRatio,
        Step::Unchanged => Resume::Unchanged,
        Step::Skip(action) => Resume::Skipped(action),
        Step::Apply(plan) => {
            if (d.hooks.apply)(hwnd, &plan, &d.label) {
                Resume::Applied(plan)
            } else {
                d.state.mark_pending();
                Resume::ApplyFailed
            }
        }
    }
}

/// Logs the outcome of a resume that was not part of a size-move end.
fn log_resume(d: &SubclassData, source: &str, resume: &Resume) {
    match resume {
        Resume::Applied(FitPlan::Resize {
            outer,
            client_w,
            client_h,
            capped,
        }) => {
            let ratio = d.state.ratio().unwrap_or(0.0);
            tracing::info!(
                label = d.label,
                source,
                client_w,
                client_h,
                top = outer.top,
                capped,
                ratio = format!("{ratio:.6}"),
                "window fitted"
            );
        }
        other => tracing::debug!(
            label = d.label,
            source,
            outcome = other.name(),
            "fit not applied"
        ),
    }
}

/// The `WM_SIZING` rewrite. Returns true only when it changed the rectangle, in which case the
/// message is answered without being forwarded. Silent on every pass-through: it runs per mouse
/// move.
fn rewrite_sizing(hwnd: HWND, d: &SubclassData, wparam: WPARAM, lparam: LPARAM) -> bool {
    let Some(ratio) = d.state.ratio() else {
        return false;
    };
    let Some(edge) = u32::try_from(wparam).ok().and_then(Edge::from_wmsz) else {
        return false;
    };
    // SAFETY: `hwnd` is the window this proc is running for, on its own thread.
    let Some(f) = (unsafe { hwnd_facts(hwnd) }) else {
        return false;
    };
    if f.maximized {
        return false;
    }
    let Some(work) = f.work else {
        return false;
    };
    let rect = lparam as *mut RECT;
    if rect.is_null() {
        return false;
    }
    // SAFETY: for `WM_SIZING`, `lparam` points to a `RECT` the system owns for this call.
    let proposed = rect_of(unsafe { *rect });
    let nc = Size {
        w: f.outer
            .right
            .saturating_sub(f.outer.left)
            .saturating_sub(f.client.w),
        h: f.outer
            .bottom
            .saturating_sub(f.outer.top)
            .saturating_sub(f.client.h),
    };
    let fitted = fit_rect(
        edge,
        proposed,
        nc,
        ratio,
        &client_bounds(f.dpi, Some(work), nc),
    );
    // SAFETY: as above; the system reads the rectangle back after the proc returns.
    unsafe {
        *rect = RECT {
            left: fitted.left,
            top: fitted.top,
            right: fitted.right,
            bottom: fitted.bottom,
        };
    }
    true
}

/// The "size-move end" line: the client as it is after any apply, and how far it is from the
/// content's shape (`client_h - ceil(client_w / ratio)`).
fn log_size_move_end(hwnd: HWND, d: &SubclassData, pending: &str) {
    // SAFETY: `hwnd` is the window this proc is running for, on its own thread.
    let Some(f) = (unsafe { hwnd_facts(hwnd) }) else {
        warn_facts_unavailable(&d.label);
        return;
    };
    let ratio_err_px = d.state.ratio().map(|r| {
        f.client
            .h
            .saturating_sub((f64::from(f.client.w) / r).ceil() as i32)
    });
    tracing::info!(
        label = d.label,
        client_w = f.client.w,
        client_h = f.client.h,
        dpi = f.dpi,
        ratio_err_px,
        pending,
        "size-move end"
    );
}

/// The subclass proc. Every message is forwarded to the original proc exactly once, except a
/// `WM_SIZING` the proc answers itself. The proc's own work runs inside `run_guarded`, never the
/// forward, so a panic is contained and cannot cause a second forward.
///
/// # Safety
///
/// Called only by the system through `SetWindowSubclass`, with `data` the pointer
/// `install_hwnd_with` leaked, which stays valid until `WM_NCDESTROY` frees it.
unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        // SAFETY: removing our own subclass from the window this proc is running for.
        unsafe { RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID) };
        // SAFETY: forwards the system's own arguments; nothing of ours runs afterwards.
        let r = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        // SAFETY: `data` came from `Box::into_raw` and, with the subclass removed and the last
        // message forwarded, nothing can reach it again.
        drop(unsafe { Box::from_raw(data as *mut SubclassData) });
        return r;
    }
    // SAFETY: `data` is valid until `WM_NCDESTROY`, handled above.
    let d = unsafe { &*(data as *const SubclassData) };
    let forward = || {
        // SAFETY: forwards the system's own arguments.
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    };
    match msg {
        WM_ENTERSIZEMOVE => {
            run_guarded(&PANIC_WARNED, || d.state.begin_size_move(), || ());
            forward()
        }
        WM_SIZING => {
            if run_guarded(
                &PANIC_WARNED,
                || rewrite_sizing(hwnd, d, wparam, lparam),
                || false,
            ) {
                1
            } else {
                forward()
            }
        }
        WM_EXITSIZEMOVE => {
            let r = forward();
            // Cleared outside the guarded work so a panic there cannot leave the drag lock stuck.
            let pending = d.state.end_size_move();
            run_guarded(
                &PANIC_WARNED,
                || {
                    (d.hooks.after_forward)(msg);
                    let outcome = if pending {
                        resume(hwnd, d).name()
                    } else {
                        "none"
                    };
                    log_size_move_end(hwnd, d, outcome);
                },
                || (),
            );
            r
        }
        WM_SIZE if wparam == SIZE_RESTORED as WPARAM => {
            let r = forward();
            run_guarded(
                &PANIC_WARNED,
                || {
                    (d.hooks.after_forward)(msg);
                    if !d.state.in_size_move() && d.state.is_pending() {
                        log_resume(d, "restored", &resume(hwnd, d));
                    }
                },
                || (),
            );
            r
        }
        WM_DPICHANGED => {
            let r = forward();
            run_guarded(
                &PANIC_WARNED,
                || {
                    (d.hooks.after_forward)(msg);
                    d.state.mark_pending();
                    if !d.state.in_size_move() {
                        log_resume(d, "dpi", &resume(hwnd, d));
                    }
                },
                || (),
            );
            r
        }
        _ => forward(),
    }
}

#[cfg(all(windows, test))]
mod hwnd_tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::ptr::{null, null_mut};
    use std::sync::Once;
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::AdjustWindowRectExForDpi;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, SystemParametersInfoW,
        SPI_GETWORKAREA, WMSZ_BOTTOM, WMSZ_BOTTOMLEFT, WMSZ_BOTTOMRIGHT, WMSZ_LEFT, WMSZ_RIGHT,
        WMSZ_TOP, WMSZ_TOPLEFT, WMSZ_TOPRIGHT, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    };

    const CLASS_NAME: windows_sys::core::PCWSTR = windows_sys::core::w!("CUT_ASPECT_TEST");

    thread_local! {
        /// Messages that reached the class proc on this thread: the forward count.
        static FORWARDS: RefCell<HashMap<u32, u32>> = RefCell::new(HashMap::new());
    }

    unsafe extern "system" fn counting_wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        FORWARDS.with(|f| *f.borrow_mut().entry(msg).or_insert(0) += 1);
        // SAFETY: the arguments are the ones the system gave this proc.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    fn register_class() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(counting_wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                // SAFETY: a null module name asks for this executable's handle.
                hInstance: unsafe { GetModuleHandleW(null()) },
                hIcon: null_mut(),
                hCursor: null_mut(),
                hbrBackground: null_mut(),
                lpszMenuName: null(),
                lpszClassName: CLASS_NAME,
            };
            // SAFETY: `class` is fully initialised and outlives the call.
            let atom = unsafe { RegisterClassW(&class) };
            assert_ne!(atom, 0, "RegisterClassW failed");
        });
    }

    const EMPTY: RECT = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    fn create_hidden(x: i32, y: i32, w: i32, h: i32) -> HWND {
        register_class();
        // SAFETY: the class is registered; no parent, menu or creation data; no `WS_VISIBLE`.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                CLASS_NAME,
                windows_sys::core::w!("aspect test"),
                WS_OVERLAPPEDWINDOW,
                x,
                y,
                w,
                h,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            )
        };
        assert!(!hwnd.is_null(), "CreateWindowExW failed: {}", unsafe {
            GetLastError()
        });
        hwnd
    }

    fn window_rect(hwnd: HWND) -> RECT {
        let mut r = EMPTY;
        // SAFETY: `hwnd` is a live window of this thread; `r` is a stack local.
        assert_ne!(unsafe { GetWindowRect(hwnd, &mut r) }, 0);
        r
    }

    fn client_size(hwnd: HWND) -> (i32, i32) {
        let mut r = EMPTY;
        // SAFETY: as in `window_rect`.
        assert_ne!(unsafe { GetClientRect(hwnd, &mut r) }, 0);
        (r.right - r.left, r.bottom - r.top)
    }

    /// `0 <= ch - ceil(cw * 640 / 980) <= 1`, in integer arithmetic.
    fn fitted(cw: i32, ch: i32) -> bool {
        (0..=1).contains(&(ch - (cw * 640 + 979) / 980))
    }

    fn to_rect(r: RECT) -> Rect {
        Rect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
    }

    /// A hidden test window with geometry taken from the host, never from constants.
    struct Fixture {
        hwnd: HWND,
        work: RECT,
        dpi: u32,
        /// The frame `AdjustWindowRectExForDpi` adds to a zero rect.
        nc: Size,
        created: RECT,
        destroyed: Cell<bool>,
    }

    impl Fixture {
        fn new() -> Fixture {
            let mut work = EMPTY;
            // SAFETY: SPI_GETWORKAREA writes one RECT through the pointer.
            let ok = unsafe {
                SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut RECT as *mut _, 0)
            };
            assert_ne!(ok, 0, "SPI_GETWORKAREA failed");
            let (ww, wh) = (work.right - work.left, work.bottom - work.top);
            assert!(
                ww >= 1000 && wh >= 760,
                "the work area is {ww}x{wh}; the HWND tests need at least 1000x760"
            );
            let probe = create_hidden(work.left, work.top, 100, 100);
            // SAFETY: `probe` is a live window.
            let dpi = unsafe { GetDpiForWindow(probe) };
            // SAFETY: `probe` is a live window of this thread.
            unsafe { DestroyWindow(probe) };
            assert!(dpi > 0, "GetDpiForWindow returned 0");
            let mut frame = EMPTY;
            // SAFETY: `frame` is a stack local.
            let ok =
                unsafe { AdjustWindowRectExForDpi(&mut frame, WS_OVERLAPPEDWINDOW, 0, 0, dpi) };
            assert_ne!(ok, 0, "AdjustWindowRectExForDpi failed");
            let nc = Size {
                w: frame.right - frame.left,
                h: frame.bottom - frame.top,
            };
            let (x, y) = (work.left + 40, work.top + 40);
            let (w, h) = (800 + nc.w, (800 * 640 + 979) / 980 + nc.h);
            let hwnd = create_hidden(x, y, w, h);
            Fixture {
                hwnd,
                work,
                dpi,
                nc,
                created: RECT {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + h,
                },
                destroyed: Cell::new(false),
            }
        }

        fn destroy(&self) {
            if !self.destroyed.replace(true) {
                // SAFETY: the window belongs to this thread and is destroyed once.
                unsafe { DestroyWindow(self.hwnd) };
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.destroy();
        }
    }

    #[test]
    fn hwnd_facts_reads_a_hidden_window() {
        let fx = Fixture::new();
        // SAFETY: `fx.hwnd` is a live window created on this thread.
        let f = unsafe { hwnd_facts(fx.hwnd) }.expect("facts for a live window");
        assert_eq!(f.outer, to_rect(fx.created));
        assert_eq!(
            Size {
                w: f.outer.right - f.outer.left - f.client.w,
                h: f.outer.bottom - f.outer.top - f.client.h
            },
            fx.nc
        );
        assert_eq!(f.client, Size { w: 800, h: 523 });
        assert_eq!(f.work, Some(to_rect(fx.work)));
        assert_eq!(f.dpi, fx.dpi);
        assert!(!f.maximized);
        assert!(!f.minimized);
    }

    #[test]
    fn apply_hwnd_resizes_without_moving() {
        let fx = Fixture::new();
        let c = fx.created;
        let plan = FitPlan::Resize {
            outer: Rect {
                left: c.left,
                top: c.top,
                right: c.left + 900 + fx.nc.w,
                bottom: c.top + 588 + fx.nc.h,
            },
            client_w: 900,
            client_h: 588,
            capped: false,
        };
        assert!(apply_hwnd(fx.hwnd, &plan, "test"));
        let r = window_rect(fx.hwnd);
        assert_eq!((r.left, r.top), (c.left, c.top));
        let (cw, ch) = client_size(fx.hwnd);
        assert_eq!(cw, 900);
        assert!(fitted(cw, ch), "client {cw}x{ch}");
    }

    #[test]
    fn apply_hwnd_moves_up_and_resizes_in_one_call() {
        let fx = Fixture::new();
        let c = fx.created;
        let top = c.top - 20;
        let plan = FitPlan::Resize {
            outer: Rect {
                left: c.left,
                top,
                right: c.left + 800 + fx.nc.w,
                bottom: top + 543 + fx.nc.h,
            },
            client_w: 800,
            client_h: 543,
            capped: false,
        };
        assert!(apply_hwnd(fx.hwnd, &plan, "test"));
        assert_eq!(window_rect(fx.hwnd).top, top);
        assert_eq!(client_size(fx.hwnd), (800, 543));
    }

    #[test]
    fn apply_hwnd_unchanged_makes_no_call() {
        let fx = Fixture::new();
        let before = window_rect(fx.hwnd);
        assert!(apply_hwnd(fx.hwnd, &FitPlan::Unchanged, "test"));
        let after = window_rect(fx.hwnd);
        assert_eq!(
            (before.left, before.top, before.right, before.bottom),
            (after.left, after.top, after.right, after.bottom)
        );
    }

    #[test]
    fn wmsz_literals_match_windows_sys() {
        let rows = [
            (WMSZ_LEFT, Edge::Left),
            (WMSZ_RIGHT, Edge::Right),
            (WMSZ_TOP, Edge::Top),
            (WMSZ_TOPLEFT, Edge::TopLeft),
            (WMSZ_TOPRIGHT, Edge::TopRight),
            (WMSZ_BOTTOM, Edge::Bottom),
            (WMSZ_BOTTOMLEFT, Edge::BottomLeft),
            (WMSZ_BOTTOMRIGHT, Edge::BottomRight),
        ];
        for (wmsz, edge) in rows {
            assert_eq!(Edge::from_wmsz(wmsz), Some(edge), "{wmsz}");
        }
    }

    // ---- Task 6: the subclass proc, driven by real messages to a hidden window ----

    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SendMessageW, SetWindowLongPtrW, GWL_STYLE, SIZE_RESTORED,
        WM_DPICHANGED, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_SIZE, WM_SIZING, WS_MAXIMIZE,
    };

    const RATIO: f64 = 980.0 / 640.0;

    fn forwards(msg: u32) -> u32 {
        FORWARDS.with(|f| f.borrow().get(&msg).copied().unwrap_or(0))
    }

    fn clear_forwards() {
        FORWARDS.with(|f| f.borrow_mut().clear());
    }

    fn rig_with(state: Arc<AspectState>, hooks: ProcHooks) -> (Fixture, Arc<AspectState>) {
        let fx = Fixture::new();
        install_hwnd_with(fx.hwnd, Arc::clone(&state), "test".into(), hooks)
            .expect("the subclass installs on a live window");
        (fx, state)
    }

    fn rig() -> (Fixture, Arc<AspectState>) {
        let state = Arc::new(AspectState::default());
        state.set_ratio(RATIO);
        rig_with(state, ProcHooks::REAL)
    }

    /// Client 800 x 600: 77 px taller than the fitted 800 x 523.
    fn off_shape(fx: &Fixture) {
        // SAFETY: the window is live and belongs to this thread.
        let ok = unsafe {
            SetWindowPos(
                fx.hwnd,
                null_mut(),
                0,
                0,
                800 + fx.nc.w,
                600 + fx.nc.h,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        assert_ne!(ok, 0, "off-shape SetWindowPos failed");
        assert_eq!(client_size(fx.hwnd), (800, 600));
    }

    fn send(fx: &Fixture, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // SAFETY: the window is live and belongs to this thread, so the message is delivered
        // synchronously to the subclass proc; every `lparam` a test passes is valid for `msg`.
        unsafe { SendMessageW(fx.hwnd, msg, wparam, lparam) }
    }

    fn assert_fitted(fx: &Fixture) {
        let (cw, ch) = client_size(fx.hwnd);
        assert!(fitted(cw, ch), "client {cw}x{ch} is not fitted");
    }

    #[test]
    fn sizing_rewrites_every_edge() {
        let (fx, _state) = rig();
        // SAFETY: the window is live and belongs to this thread.
        let f = unsafe { hwnd_facts(fx.hwnd) }.expect("facts");
        let nc = Size {
            w: f.outer.right - f.outer.left - f.client.w,
            h: f.outer.bottom - f.outer.top - f.client.h,
        };
        let b = client_bounds(f.dpi, f.work, nc);
        let rows = [
            (WMSZ_LEFT, Edge::Left),
            (WMSZ_RIGHT, Edge::Right),
            (WMSZ_TOP, Edge::Top),
            (WMSZ_TOPLEFT, Edge::TopLeft),
            (WMSZ_TOPRIGHT, Edge::TopRight),
            (WMSZ_BOTTOM, Edge::Bottom),
            (WMSZ_BOTTOMLEFT, Edge::BottomLeft),
            (WMSZ_BOTTOMRIGHT, Edge::BottomRight),
        ];
        let created = to_rect(fx.created);
        for (wmsz, edge) in rows {
            let drags_left = matches!(edge, Edge::Left | Edge::TopLeft | Edge::BottomLeft);
            let drags_right = matches!(edge, Edge::Right | Edge::TopRight | Edge::BottomRight);
            let drags_top = matches!(edge, Edge::Top | Edge::TopLeft | Edge::TopRight);
            let drags_bottom = matches!(edge, Edge::Bottom | Edge::BottomLeft | Edge::BottomRight);
            let mut grown = created;
            if drags_left {
                grown.left -= 40;
            }
            if drags_right {
                grown.right += 40;
            }
            if drags_top {
                grown.top -= 40;
            }
            if drags_bottom {
                grown.bottom += 40;
            }
            let mut rect = RECT {
                left: grown.left,
                top: grown.top,
                right: grown.right,
                bottom: grown.bottom,
            };
            clear_forwards();
            let r = send(
                &fx,
                WM_SIZING,
                wmsz as WPARAM,
                &mut rect as *mut RECT as LPARAM,
            );
            assert_eq!(r, 1, "{edge:?}: TRUE means the rectangle was changed");
            assert_eq!(
                forwards(WM_SIZING),
                0,
                "{edge:?}: a handled WM_SIZING is not forwarded"
            );
            let out = to_rect(rect);
            // Wiring: the proc applies the pure core with the window's own facts.
            assert_eq!(out, fit_rect(edge, grown, nc, RATIO, &b), "{edge:?}");
            // Independent geometry: the oracle frame, the held edges, the dragged edge's travel.
            let (cw, ch) = (
                out.right - out.left - fx.nc.w,
                out.bottom - out.top - fx.nc.h,
            );
            assert!(fitted(cw, ch), "{edge:?}: client {cw}x{ch}");
            // The anchor: dragging a left or top edge holds the right or bottom one; any other
            // drag holds the left or top one. The other axis follows from the ratio.
            if drags_left {
                assert_eq!(out.right, grown.right, "{edge:?}: right held");
            } else {
                assert_eq!(out.left, grown.left, "{edge:?}: left held");
            }
            if drags_top {
                assert_eq!(out.bottom, grown.bottom, "{edge:?}: bottom held");
            } else {
                assert_eq!(out.top, grown.top, "{edge:?}: top held");
            }
            let moved_out = (drags_left && out.left < created.left)
                || (drags_right && out.right > created.right)
                || (drags_top && out.top < created.top)
                || (drags_bottom && out.bottom > created.bottom);
            assert!(moved_out, "{edge:?}: no dragged edge moved outward");
        }
    }

    #[test]
    fn sizing_passes_through_without_a_ratio() {
        let (fx, _state) = rig_with(Arc::new(AspectState::default()), ProcHooks::REAL);
        let c = fx.created;
        let mut rect = RECT {
            left: c.left,
            top: c.top,
            right: c.right + 40,
            bottom: c.bottom,
        };
        clear_forwards();
        send(
            &fx,
            WM_SIZING,
            WMSZ_RIGHT as WPARAM,
            &mut rect as *mut RECT as LPARAM,
        );
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (c.left, c.top, c.right + 40, c.bottom)
        );
        assert_eq!(forwards(WM_SIZING), 1);
    }

    #[test]
    fn exit_size_move_applies_pending_synchronously() {
        let (fx, state) = rig();
        send(&fx, WM_ENTERSIZEMOVE, 0, 0);
        assert!(state.in_size_move());
        off_shape(&fx);
        state.mark_pending();
        clear_forwards();
        send(&fx, WM_EXITSIZEMOVE, 0, 0);
        assert!(!state.in_size_move());
        assert!(!state.is_pending());
        assert_fitted(&fx);
        assert_eq!(forwards(WM_EXITSIZEMOVE), 1);
    }

    #[test]
    fn size_restored_applies_pending() {
        let (fx, state) = rig();
        off_shape(&fx);
        state.mark_pending();
        let lparam = 800isize | (600isize << 16);
        send(&fx, WM_SIZE, SIZE_RESTORED as WPARAM, lparam);
        assert_fitted(&fx);
        assert!(!state.is_pending());
    }

    #[test]
    fn dpi_changed_applies_when_not_in_a_size_move() {
        let (fx, state) = rig();
        off_shape(&fx);
        let mut suggested = window_rect(fx.hwnd);
        let dpi = fx.dpi as usize;
        clear_forwards();
        send(
            &fx,
            WM_DPICHANGED,
            dpi | (dpi << 16),
            &mut suggested as *mut RECT as LPARAM,
        );
        assert_fitted(&fx);
        assert!(!state.is_pending());
        assert_eq!(forwards(WM_DPICHANGED), 1);
    }

    #[test]
    fn dpi_changed_in_a_size_move_marks_pending() {
        let (fx, state) = rig();
        send(&fx, WM_ENTERSIZEMOVE, 0, 0);
        off_shape(&fx);
        let mut suggested = window_rect(fx.hwnd);
        let dpi = fx.dpi as usize;
        send(
            &fx,
            WM_DPICHANGED,
            dpi | (dpi << 16),
            &mut suggested as *mut RECT as LPARAM,
        );
        assert_eq!(client_size(fx.hwnd), (800, 600));
        assert!(state.is_pending());
        send(&fx, WM_EXITSIZEMOVE, 0, 0);
        assert_fitted(&fx);
        assert!(!state.is_pending());
    }

    #[test]
    fn apply_failure_in_the_proc_keeps_pending() {
        let state = Arc::new(AspectState::default());
        state.set_ratio(RATIO);
        let hooks = ProcHooks {
            apply: |_, _, _| false,
            after_forward: ProcHooks::REAL.after_forward,
        };
        let (fx, state) = rig_with(state, hooks);
        off_shape(&fx);
        state.mark_pending();
        send(&fx, WM_EXITSIZEMOVE, 0, 0);
        assert_eq!(client_size(fx.hwnd), (800, 600));
        assert!(state.is_pending());
    }

    #[test]
    fn a_panic_after_the_forward_forwards_once() {
        let state = Arc::new(AspectState::default());
        state.set_ratio(RATIO);
        let hooks = ProcHooks {
            apply: apply_hwnd,
            after_forward: |_| panic!("test"),
        };
        let (fx, _state) = rig_with(state, hooks);
        clear_forwards();
        send(&fx, WM_EXITSIZEMOVE, 0, 0);
        assert_eq!(forwards(WM_EXITSIZEMOVE), 1);
        clear_forwards();
        send(&fx, WM_SIZE, SIZE_RESTORED as WPARAM, 0);
        assert_eq!(forwards(WM_SIZE), 1);
    }

    #[test]
    fn a_panic_in_a_size_move_end_still_ends_the_size_move() {
        let state = Arc::new(AspectState::default());
        state.set_ratio(RATIO);
        let hooks = ProcHooks {
            apply: apply_hwnd,
            after_forward: |_| panic!("test"),
        };
        let (fx, state) = rig_with(state, hooks);
        send(&fx, WM_ENTERSIZEMOVE, 0, 0);
        assert!(state.in_size_move());
        send(&fx, WM_EXITSIZEMOVE, 0, 0);
        assert!(
            !state.in_size_move(),
            "a stuck size-move flag would defer every later fit"
        );
    }

    #[test]
    fn zoomed_window_keeps_pending() {
        let (fx, state) = rig();
        // SAFETY: the window is live and belongs to this thread.
        unsafe {
            let style = GetWindowLongPtrW(fx.hwnd, GWL_STYLE);
            SetWindowLongPtrW(fx.hwnd, GWL_STYLE, style | WS_MAXIMIZE as isize);
        }
        off_shape(&fx);
        state.mark_pending();
        send(&fx, WM_EXITSIZEMOVE, 0, 0);
        assert_eq!(client_size(fx.hwnd), (800, 600));
        assert!(state.is_pending());
    }

    #[test]
    fn destroy_frees_subclass_data() {
        let (fx, state) = rig();
        assert_eq!(Arc::strong_count(&state), 2);
        fx.destroy();
        assert_eq!(Arc::strong_count(&state), 1);
    }

    #[test]
    fn a_failed_install_returns_the_error_and_frees_the_data() {
        let state = Arc::new(AspectState::default());
        let r = install_hwnd(null_mut(), Arc::clone(&state), "test".into());
        assert!(r.is_err(), "SetWindowSubclass on a null window must fail");
        assert_eq!(Arc::strong_count(&state), 1);
    }
}
