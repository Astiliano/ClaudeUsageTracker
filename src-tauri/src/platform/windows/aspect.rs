//! The Windows side of the aspect lock: facts read straight from the HWND, and the one apply
//! (`apply_hwnd`) shared by the `set_content_height` command and the subclass proc.

use crate::error::{AppError, AppResult};
use crate::window_aspect::{FitPlan, Rect, Size, WindowFacts};
use windows_sys::Win32::Foundation::{GetLastError, HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowRect, IsIconic, IsZoomed, SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOZORDER,
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

#[cfg(all(windows, test))]
mod hwnd_tests {
    use super::*;
    use crate::window_aspect::Edge;
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
}
