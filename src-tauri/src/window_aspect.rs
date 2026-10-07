//! The window's content-hugging aspect lock: constants, the shared state and the pure, portable
//! decisions behind the `set_content_height` command and the Windows subclass proc.
//!
//! Everything here is physical px in `i32` unless stated, and compiles and tests on every
//! platform. No `clamp` (it panics when min > max), no `unwrap`, no `expect` outside tests: bounds
//! use ordered `min`/`max` steps, and float-to-int conversions go through an explicit
//! `floor`/`ceil`/`round`.

use crate::error::{AppError, AppResult};
use serde::Serialize;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Logical width of the layout; equals tauri.conf.json `width` and TS `BASE_WIDTH`.
pub const BASE_WIDTH_PX: f64 = 980.0;
/// Logical minimum width; equals tauri.conf.json `minWidth` (TS `BASE_WIDTH * ZOOM_MIN`).
pub const MIN_WIDTH_PX: f64 = 735.0;
/// Largest zoom; equals TS `ZOOM_MAX`.
pub const ZOOM_MAX: f64 = 2.5;
/// Smallest content height (local px) the command accepts.
pub const MIN_CONTENT_H: f64 = 80.0;
/// Largest content height (local px) the command accepts.
pub const MAX_CONTENT_H: f64 = 10_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    fn width(&self) -> i32 {
        self.right.saturating_sub(self.left)
    }

    fn height(&self) -> i32 {
        self.bottom.saturating_sub(self.top)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub w: i32,
    pub h: i32,
}

/// What the aspect lock remembers, independent of any window: the window is destroyed on
/// close-to-tray and rebuilt, and the rebuilt window is fitted from the stored ratio.
#[derive(Default)]
pub struct AspectState {
    /// `f64` bits of `BASE_WIDTH_PX / content_h`; 0 means unknown.
    ratio_bits: AtomicU64,
    in_size_move: AtomicBool,
    pending_fit: AtomicBool,
}

impl AspectState {
    pub fn ratio(&self) -> Option<f64> {
        match self.ratio_bits.load(Ordering::SeqCst) {
            0 => None,
            bits => Some(f64::from_bits(bits)),
        }
    }

    pub fn set_ratio(&self, ratio: f64) {
        self.ratio_bits.store(ratio.to_bits(), Ordering::SeqCst);
    }

    pub fn begin_size_move(&self) {
        self.in_size_move.store(true, Ordering::SeqCst);
    }

    /// Clears the size-move flag and says whether a fit is pending.
    pub fn end_size_move(&self) -> bool {
        self.in_size_move.store(false, Ordering::SeqCst);
        self.is_pending()
    }

    pub fn in_size_move(&self) -> bool {
        self.in_size_move.load(Ordering::SeqCst)
    }

    pub fn is_pending(&self) -> bool {
        self.pending_fit.load(Ordering::SeqCst)
    }

    pub fn mark_pending(&self) {
        self.pending_fit.store(true, Ordering::SeqCst);
    }

    /// Clears the pending flag and returns what it was.
    pub fn take_pending(&self) -> bool {
        self.pending_fit.swap(false, Ordering::SeqCst)
    }
}

/// `Ok(h)` when `h` is finite and within `[MIN_CONTENT_H, MAX_CONTENT_H]`.
pub fn validate_content_h(h: f64) -> AppResult<f64> {
    if h.is_finite() && (MIN_CONTENT_H..=MAX_CONTENT_H).contains(&h) {
        Ok(h)
    } else {
        Err(AppError::OutOfRange(format!(
            "content height {h} is outside {MIN_CONTENT_H}..={MAX_CONTENT_H}"
        )))
    }
}

/// Validates a report, then stores `BASE_WIDTH_PX / content_h`. A rejection stores nothing.
pub fn accept_report(state: &AspectState, content_h: f64) -> AppResult<f64> {
    let h = validate_content_h(content_h)?;
    state.set_ratio(BASE_WIDTH_PX / h);
    Ok(h)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitAction {
    Fit,
    Defer,
    SkipMaximized,
    SkipMinimized,
}

/// Precedence: minimized, then maximized, then in a size-move, else `Fit`.
pub fn decide_fit(maximized: bool, minimized: bool, in_size_move: bool) -> FitAction {
    if minimized {
        FitAction::SkipMinimized
    } else if maximized {
        FitAction::SkipMaximized
    } else if in_size_move {
        FitAction::Defer
    } else {
        FitAction::Fit
    }
}

/// `ceil(min_logical * dpi / 96)`.
pub fn min_client_px(min_logical: f64, dpi: u32) -> i32 {
    (min_logical * f64::from(dpi) / 96.0).ceil() as i32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientBounds {
    pub min_w: i32,
    pub max_w: i32,
    pub min_h: i32,
    pub max_h: i32,
}

/// The client-size bounds at `dpi` on a monitor with work area `work` and a frame of `nc`.
pub fn client_bounds(dpi: u32, work: Option<Rect>, nc: Size) -> ClientBounds {
    let min_w = min_client_px(MIN_WIDTH_PX, dpi);
    let min_h = min_client_px(MIN_CONTENT_H * MIN_WIDTH_PX / BASE_WIDTH_PX, dpi);
    let zoom_max_w = (BASE_WIDTH_PX * ZOOM_MAX * f64::from(dpi) / 96.0).floor() as i32;
    match work {
        Some(w) => ClientBounds {
            min_w,
            max_w: w.width().saturating_sub(nc.w).min(zoom_max_w),
            min_h,
            max_h: w.height().saturating_sub(nc.h),
        },
        None => ClientBounds {
            min_w,
            max_w: zoom_max_w,
            min_h,
            max_h: i32::MAX,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientFit {
    pub w: i32,
    pub h: i32,
    pub capped: bool,
}

/// The one definition of the capped state: the minimum width wins, then the height cap applies
/// and the ratio yields.
pub fn fit_client(target_w: f64, ratio: f64, b: &ClientBounds) -> ClientFit {
    // The width that keeps the ratio at the height cap.
    let upper = b.max_w.min((f64::from(b.max_h) * ratio).floor() as i32);
    // `f64 as i32` of NaN is 0; written out so the minimum step below visibly lifts it.
    let rounded = if target_w.is_nan() {
        0
    } else {
        target_w.round() as i32
    };
    let mut w = rounded;
    if w > upper {
        w = upper;
    }
    if w < b.min_w {
        w = b.min_w;
    }
    let want_h = (f64::from(w) / ratio).ceil() as i32;
    let h = want_h.max(b.min_h).min(b.max_h);
    ClientFit {
        w,
        h,
        capped: rounded > upper || want_h > b.max_h,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowFacts {
    pub client: Size,
    pub outer: Rect,
    pub work: Option<Rect>,
    pub dpi: u32,
    pub maximized: bool,
    pub minimized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitPlan {
    Unchanged,
    Resize {
        outer: Rect,
        client_w: i32,
        client_h: i32,
        capped: bool,
    },
}

/// The resize that gives the window the content's shape at its current client width, moving it up
/// only by the growth the fit adds, and only as far as that growth crosses the work area's bottom.
pub fn plan_fit(f: &WindowFacts, ratio: f64) -> FitPlan {
    let nc = Size {
        w: f.outer.width().saturating_sub(f.client.w),
        h: f.outer.height().saturating_sub(f.client.h),
    };
    let b = client_bounds(f.dpi, f.work, nc);
    let fit = fit_client(f64::from(f.client.w), ratio, &b);
    let shortfall = f.client.h.saturating_sub(fit.h);
    if fit.w == f.client.w && (0..=1).contains(&shortfall) {
        return FitPlan::Unchanged;
    }
    let grow = fit.h.saturating_sub(f.client.h).max(0);
    let overflow = match f.work {
        Some(w) => f
            .outer
            .top
            .saturating_add(nc.h)
            .saturating_add(fit.h)
            .saturating_sub(w.bottom)
            .max(0),
        None => 0,
    };
    let raised = f.outer.top.saturating_sub(grow.min(overflow));
    let top = match f.work {
        Some(w) => raised.max(w.top),
        None => raised,
    };
    let left = f.outer.left;
    FitPlan::Resize {
        outer: Rect {
            left,
            top,
            right: left.saturating_add(fit.w).saturating_add(nc.w),
            bottom: top.saturating_add(fit.h).saturating_add(nc.h),
        },
        client_w: fit.w,
        client_h: fit.h,
        capped: fit.capped,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    NoRatio,
    Skip(FitAction),
    Unchanged,
    Apply(FitPlan),
}

/// The one decision path, shared by the command, the ready hook and the proc. Every non-`Fit`
/// action leaves a pending fit; a `Fit` takes the pending flag before planning, so the `WM_SIZE`
/// our own apply causes finds nothing pending.
pub fn step(state: &AspectState, f: &WindowFacts) -> Step {
    let Some(ratio) = state.ratio() else {
        return Step::NoRatio;
    };
    match decide_fit(f.maximized, f.minimized, state.in_size_move()) {
        FitAction::Fit => {
            state.take_pending();
            match plan_fit(f, ratio) {
                FitPlan::Unchanged => Step::Unchanged,
                plan => Step::Apply(plan),
            }
        }
        skip => {
            state.mark_pending();
            Step::Skip(skip)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    TopLeft,
    TopRight,
    Bottom,
    BottomLeft,
    BottomRight,
}

impl Edge {
    /// The Win32 `WMSZ_*` values 1-8 as literals, so the mapping is portable; others give `None`.
    pub fn from_wmsz(v: u32) -> Option<Edge> {
        match v {
            1 => Some(Edge::Left),
            2 => Some(Edge::Right),
            3 => Some(Edge::Top),
            4 => Some(Edge::TopLeft),
            5 => Some(Edge::TopRight),
            6 => Some(Edge::Bottom),
            7 => Some(Edge::BottomLeft),
            8 => Some(Edge::BottomRight),
            _ => None,
        }
    }
}

/// The `WM_SIZING` rectangle after a drag of `edge` is held to `ratio` within the bounds `b`.
/// Dragged edges move and the opposite edges keep their coordinate; the window is never moved up
/// beyond what the dragged top edge implies.
pub fn fit_rect(edge: Edge, rect: Rect, nc: Size, ratio: f64, b: &ClientBounds) -> Rect {
    let cw = f64::from(rect.width().saturating_sub(nc.w));
    let ch = f64::from(rect.height().saturating_sub(nc.h));
    let target_w = match edge {
        Edge::Left | Edge::Right => cw,
        Edge::Top | Edge::Bottom => ch * ratio,
        Edge::TopLeft | Edge::TopRight | Edge::BottomLeft | Edge::BottomRight => cw.max(ch * ratio),
    };
    let fit = fit_client(target_w, ratio, b);
    let outer_w = fit.w.saturating_add(nc.w);
    let outer_h = fit.h.saturating_add(nc.h);
    let mut out = rect;
    if matches!(edge, Edge::Left | Edge::TopLeft | Edge::BottomLeft) {
        out.left = rect.right.saturating_sub(outer_w);
    } else {
        out.right = rect.left.saturating_add(outer_w);
    }
    if matches!(edge, Edge::Top | Edge::TopLeft | Edge::TopRight) {
        out.top = rect.bottom.saturating_sub(outer_h);
    } else {
        out.bottom = rect.top.saturating_add(outer_h);
    }
    out
}

/// Runs `body`, containing a panic: on a panic it logs one WARN per `warned` flag and returns
/// `fallback()`. The Windows proc wraps only its own work in this, never the forward to
/// `DefSubclassProc`, so a panic can never cause a second forward.
pub fn run_guarded<T>(
    warned: &AtomicBool,
    body: impl FnOnce() -> T,
    fallback: impl FnOnce() -> T,
) -> T {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(v) => v,
        Err(_) => {
            if !warned.swap(true, Ordering::SeqCst) {
                tracing::warn!("window aspect subclass panicked; forwarding");
            }
            fallback()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FitOutcome {
    Applied,
    AlreadyFitted,
    Capped,
    SkippedMaximized,
    SkippedMinimized,
    Deferred,
}

/// The outcome the command reports for a step; `NoRatio` has none because the command always
/// stores the ratio first.
pub fn outcome_of(step: &Step) -> Option<FitOutcome> {
    match step {
        Step::NoRatio => None,
        Step::Unchanged => Some(FitOutcome::AlreadyFitted),
        Step::Apply(FitPlan::Resize { capped: true, .. }) => Some(FitOutcome::Capped),
        Step::Apply(_) => Some(FitOutcome::Applied),
        Step::Skip(FitAction::SkipMaximized) => Some(FitOutcome::SkippedMaximized),
        Step::Skip(FitAction::SkipMinimized) => Some(FitOutcome::SkippedMinimized),
        Step::Skip(FitAction::Defer) => Some(FitOutcome::Deferred),
        Step::Skip(FitAction::Fit) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    const RATIO: f64 = 980.0 / 640.0;
    const NC: Size = Size { w: 16, h: 39 };
    const WORK: Rect = Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1032,
    };

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    fn bounds() -> ClientBounds {
        client_bounds(96, Some(WORK), NC)
    }

    fn state_with_ratio() -> AspectState {
        let s = AspectState::default();
        s.set_ratio(RATIO);
        s
    }

    /// A normal window whose client is `cw` x `ch`, outer left 100, outer top `top`.
    fn facts(cw: i32, ch: i32, top: i32) -> WindowFacts {
        WindowFacts {
            client: Size { w: cw, h: ch },
            outer: rect(100, top, 100 + cw + NC.w, top + ch + NC.h),
            work: Some(WORK),
            dpi: 96,
            maximized: false,
            minimized: false,
        }
    }

    fn resize(left: i32, top: i32, cw: i32, ch: i32, capped: bool) -> FitPlan {
        FitPlan::Resize {
            outer: rect(left, top, left + cw + NC.w, top + ch + NC.h),
            client_w: cw,
            client_h: ch,
            capped,
        }
    }

    const ALL_EDGES: [Edge; 8] = [
        Edge::Left,
        Edge::Right,
        Edge::Top,
        Edge::TopLeft,
        Edge::TopRight,
        Edge::Bottom,
        Edge::BottomLeft,
        Edge::BottomRight,
    ];

    fn out_of_range<T: std::fmt::Debug>(r: AppResult<T>) -> bool {
        matches!(r, Err(AppError::OutOfRange(_)))
    }

    // ---- validation and state ----

    #[test]
    fn validate_content_h_accepts_the_range() {
        for h in [80.0, 640.0, 10_000.0] {
            assert!(matches!(validate_content_h(h), Ok(v) if v == h), "{h}");
        }
    }

    #[test]
    fn validate_content_h_rejects() {
        for h in [
            79.9,
            10_000.1,
            0.0,
            -1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            1e-300,
        ] {
            assert!(out_of_range(validate_content_h(h)), "{h}");
        }
    }

    #[test]
    fn default_ratio_is_unknown() {
        assert_eq!(AspectState::default().ratio(), None);
    }

    #[test]
    fn rejected_report_keeps_old_ratio() {
        let s = AspectState::default();
        assert!(accept_report(&s, 640.0).is_ok());
        assert!(out_of_range(accept_report(&s, 0.0)));
        assert_eq!(s.ratio(), Some(980.0 / 640.0));
    }

    #[test]
    fn end_size_move_reports_pending_and_step_takes_it_once() {
        let s = state_with_ratio();
        s.begin_size_move();
        assert!(s.in_size_move());
        s.mark_pending();
        assert!(s.end_size_move());
        assert!(!s.in_size_move());
        let off_shape = facts(1000, 700, 100);
        assert!(matches!(step(&s, &off_shape), Step::Apply(_)));
        assert!(!s.is_pending());
        // The same facts again plan the same resize but must not re-mark pending.
        assert!(matches!(step(&s, &off_shape), Step::Apply(_)));
        assert!(!s.is_pending());
    }

    // ---- decide_fit and step ----

    #[test]
    fn decide_fit_table() {
        use FitAction::*;
        // (maximized, minimized, in_size_move) -> action
        let rows = [
            (false, false, false, Fit),
            (false, false, true, Defer),
            (false, true, false, SkipMinimized),
            (false, true, true, SkipMinimized),
            (true, false, false, SkipMaximized),
            (true, false, true, SkipMaximized),
            (true, true, false, SkipMinimized),
            (true, true, true, SkipMinimized),
        ];
        for (max, min, moving, want) in rows {
            assert_eq!(decide_fit(max, min, moving), want, "{max} {min} {moving}");
        }
    }

    #[test]
    fn step_table() {
        // No ratio.
        let none = AspectState::default();
        assert_eq!(step(&none, &facts(1000, 654, 100)), Step::NoRatio);
        assert!(!none.is_pending());

        // Minimized, maximized, in a size-move: each skips and leaves a pending fit.
        let mut minimized = facts(1000, 700, 100);
        minimized.minimized = true;
        let s = state_with_ratio();
        assert_eq!(step(&s, &minimized), Step::Skip(FitAction::SkipMinimized));
        assert!(s.is_pending());

        let mut maximized = facts(1000, 700, 100);
        maximized.maximized = true;
        let s = state_with_ratio();
        assert_eq!(step(&s, &maximized), Step::Skip(FitAction::SkipMaximized));
        assert!(s.is_pending());

        let s = state_with_ratio();
        s.begin_size_move();
        assert_eq!(
            step(&s, &facts(1000, 700, 100)),
            Step::Skip(FitAction::Defer)
        );
        assert!(s.is_pending());

        // Normal and within 1 px: Unchanged, pending cleared.
        let s = state_with_ratio();
        s.mark_pending();
        assert_eq!(step(&s, &facts(1000, 655, 100)), Step::Unchanged);
        assert!(!s.is_pending());

        // Normal and off-shape: Apply, pending cleared.
        let s = state_with_ratio();
        s.mark_pending();
        assert_eq!(
            step(&s, &facts(1000, 700, 100)),
            Step::Apply(resize(100, 100, 1000, 654, false))
        );
        assert!(!s.is_pending());
    }

    // ---- bounds ----

    #[test]
    fn min_client_px_table() {
        assert_eq!(min_client_px(735.0, 96), 735);
        assert_eq!(min_client_px(735.0, 144), 1103);
        assert_eq!(min_client_px(735.0, 120), 919);
    }

    #[test]
    fn client_bounds_table() {
        assert_eq!(
            bounds(),
            ClientBounds {
                min_w: 735,
                max_w: 1904,
                min_h: 60,
                max_h: 993
            }
        );
        let big = rect(0, 0, 3840, 2112);
        assert_eq!(client_bounds(96, Some(big), NC).max_w, 2450);
        let none = client_bounds(96, None, NC);
        assert_eq!(none.max_h, i32::MAX);
        assert_eq!(none.max_w, 2450);
    }

    // ---- fit_client ----

    #[test]
    fn fit_client_table() {
        let b = bounds();
        // Inside the bounds.
        assert_eq!(
            fit_client(1000.0, RATIO, &b),
            ClientFit {
                w: 1000,
                h: 654,
                capped: false
            }
        );
        // Above `upper` (floor(993 * ratio) = 1520): upper and capped.
        assert_eq!(
            fit_client(1800.0, RATIO, &b),
            ClientFit {
                w: 1520,
                h: 993,
                capped: true
            }
        );
        // Height-capped with the ratio kept.
        let short = ClientBounds { max_h: 500, ..b };
        assert_eq!(
            fit_client(1000.0, RATIO, &short),
            ClientFit {
                w: 765,
                h: 500,
                capped: true
            }
        );
        // Crossed (min_w 735 > floor(400 * ratio) = 612): min_w wins, the height cap applies.
        let crossed = ClientBounds { max_h: 400, ..b };
        assert_eq!(
            fit_client(1000.0, RATIO, &crossed),
            ClientFit {
                w: 735,
                h: 400,
                capped: true
            }
        );
        // A NaN target gives min_w.
        assert_eq!(fit_client(f64::NAN, RATIO, &b).w, 735);
        // A ratio far outside the validated range: want_h 8 < min_h 60.
        assert_eq!(
            fit_client(735.0, 100.0, &b),
            ClientFit {
                w: 735,
                h: 60,
                capped: false
            }
        );
    }

    // ---- plan_fit ----

    #[test]
    fn plan_fit_table() {
        // Within 1 px.
        assert_eq!(plan_fit(&facts(1000, 654, 100), RATIO), FitPlan::Unchanged);
        assert_eq!(plan_fit(&facts(1000, 655, 100), RATIO), FitPlan::Unchanged);
        // 1 px short: never shorter than the content.
        assert_eq!(
            plan_fit(&facts(1000, 653, 100), RATIO),
            resize(100, 100, 1000, 654, false)
        );
        // A grow with room keeps the top.
        assert_eq!(
            plan_fit(&facts(1000, 400, 100), RATIO),
            resize(100, 100, 1000, 654, false)
        );
        // A grow past the work bottom moves up by the overflow (61): bottom 400 + 39 + 654 = 1093.
        assert_eq!(
            plan_fit(&facts(1000, 400, 400), RATIO),
            resize(100, 339, 1000, 654, false)
        );
        // a same-size window below the work area is Unchanged.
        assert_eq!(plan_fit(&facts(1000, 654, 900), RATIO), FitPlan::Unchanged);
        // a grow from an already-overflowing position moves up by the growth only.
        assert_eq!(
            plan_fit(&facts(1000, 654, 500), 980.0 / 660.0),
            resize(100, 480, 1000, 674, false)
        );
        // An overflow beyond the room: top = work.top, the cap and capped.
        assert_eq!(
            plan_fit(&facts(1800, 300, 200), RATIO),
            resize(100, 0, 1520, 993, true)
        );
        // A shrink.
        assert_eq!(
            plan_fit(&facts(1200, 900, 100), RATIO),
            resize(100, 100, 1200, 784, false)
        );
        // A width below the bounds after a DPI change (144 dpi: min_w 1103) is resized into them.
        let mut f = facts(700, 500, 100);
        f.dpi = 144;
        assert_eq!(plan_fit(&f, RATIO), resize(100, 100, 1103, 721, false));
    }

    // ---- fit_rect ----

    #[test]
    fn fit_rect_worked_case() {
        let out = fit_rect(Edge::Right, rect(100, 100, 1116, 800), NC, RATIO, &bounds());
        assert_eq!(out, rect(100, 100, 1116, 793));
        assert_eq!(
            (out.right - out.left - NC.w, out.bottom - out.top - NC.h),
            (1000, 654)
        );
    }

    #[test]
    fn fit_rect_edges_anchor() {
        let b = bounds();
        // Proposed client 1100 x 660. The width term gives 1100 x 719 (outer 1116 x 758); the
        // height term gives 1011 x 661 (outer 1027 x 700).
        let a = rect(200, 100, 1316, 799);
        let rows = [
            (Edge::Left, rect(200, 100, 1316, 858)),
            (Edge::Right, rect(200, 100, 1316, 858)),
            (Edge::Top, rect(200, 99, 1227, 799)),
            (Edge::Bottom, rect(200, 100, 1227, 800)),
            (Edge::BottomRight, rect(200, 100, 1316, 858)),
            (Edge::BottomLeft, rect(200, 100, 1316, 858)),
            (Edge::TopLeft, rect(200, 41, 1316, 799)),
            (Edge::TopRight, rect(200, 41, 1316, 799)),
        ];
        for (edge, want) in rows {
            assert_eq!(fit_rect(edge, a, NC, RATIO, &b), want, "{edge:?}");
        }
        // Corners cover: proposed client 884 x 760 gives 1164 x 761 (outer 1180 x 800).
        let c = rect(200, 100, 1100, 899);
        let rows = [
            (Edge::BottomRight, rect(200, 100, 1380, 900)),
            (Edge::BottomLeft, rect(-80, 100, 1100, 900)),
            (Edge::TopLeft, rect(-80, 99, 1100, 899)),
            (Edge::TopRight, rect(200, 99, 1380, 899)),
        ];
        for (edge, want) in rows {
            assert_eq!(fit_rect(edge, c, NC, RATIO, &b), want, "{edge:?}");
        }
    }

    #[test]
    fn fit_rect_min_width() {
        let b = bounds();
        // Proposed client 584 x 461, narrower than 735.
        let r = rect(500, 300, 1100, 800);
        for edge in ALL_EDGES {
            let out = fit_rect(edge, r, NC, RATIO, &b);
            assert_eq!(out.right - out.left - NC.w, 735, "{edge:?} width");
            assert_eq!(out.bottom - out.top - NC.h, 480, "{edge:?} height");
            let moves_left = matches!(edge, Edge::Left | Edge::TopLeft | Edge::BottomLeft);
            let moves_top = matches!(edge, Edge::Top | Edge::TopLeft | Edge::TopRight);
            if moves_left {
                assert_eq!(out.right, r.right, "{edge:?}");
            } else {
                assert_eq!(out.left, r.left, "{edge:?}");
            }
            if moves_top {
                assert_eq!(out.bottom, r.bottom, "{edge:?}");
            } else {
                assert_eq!(out.top, r.top, "{edge:?}");
            }
        }
    }

    #[test]
    fn fit_rect_height_cap() {
        let b = bounds();
        // Every edge proposes a client wider than floor(993 * ratio) = 1520: 1684 x 1100.
        let r = rect(100, 0, 1800, 1139);
        // The fit is 1520 x 993, outer 1536 x 1032.
        let rows = [
            (Edge::Left, rect(264, 0, 1800, 1032)),
            (Edge::Right, rect(100, 0, 1636, 1032)),
            (Edge::Top, rect(100, 107, 1636, 1139)),
            (Edge::Bottom, rect(100, 0, 1636, 1032)),
            (Edge::TopLeft, rect(264, 107, 1800, 1139)),
            (Edge::TopRight, rect(100, 107, 1636, 1139)),
            (Edge::BottomLeft, rect(264, 0, 1800, 1032)),
            (Edge::BottomRight, rect(100, 0, 1636, 1032)),
        ];
        for (edge, want) in rows {
            let out = fit_rect(edge, r, NC, RATIO, &b);
            assert_eq!(out, want, "{edge:?}");
            assert_eq!(out.bottom - out.top - NC.h, b.max_h, "{edge:?}");
        }
    }

    #[test]
    fn fit_rect_crossed_bounds() {
        let crossed = ClientBounds {
            max_h: 400,
            ..bounds()
        };
        let r = rect(100, 100, 1100, 600);
        for edge in ALL_EDGES {
            let out = fit_rect(edge, r, NC, RATIO, &crossed);
            assert_eq!(out.right - out.left - NC.w, 735, "{edge:?} width");
            assert_eq!(out.bottom - out.top - NC.h, 400, "{edge:?} height");
        }
    }

    #[test]
    fn fit_rect_zoom_max_cap() {
        let big = rect(0, 0, 3840, 2112);
        let b = client_bounds(96, Some(big), NC);
        let out = fit_rect(Edge::Right, rect(0, 0, 3016, 1539), NC, RATIO, &b);
        assert_eq!(out, rect(0, 0, 2466, 1639));
        assert_eq!(out.right - out.left - NC.w, 2450);
    }

    #[test]
    fn fit_rect_keeps_ratio_within_1px() {
        let b = bounds();
        for ratio in [980.0 / 640.0, 980.0 / 300.0, 980.0 / 1200.0] {
            for edge in ALL_EDGES {
                for cw in (735..=2400).step_by(15) {
                    let ch = (cw as f64 / ratio).round() as i32 + 7;
                    let r = rect(100, 100, 100 + cw + NC.w, 100 + ch + NC.h);
                    let out = fit_rect(edge, r, NC, ratio, &b);
                    let (fw, fh) = (out.right - out.left - NC.w, out.bottom - out.top - NC.h);
                    if fh == b.max_h || fw == b.max_w {
                        continue; // capped: the ratio yields
                    }
                    let want = (fw as f64 / ratio).ceil() as i32;
                    assert!(
                        (0..=1).contains(&(fh - want)),
                        "{edge:?} ratio {ratio} cw {cw}: fit {fw}x{fh}, ceil {want}"
                    );
                }
            }
        }
    }

    #[test]
    fn edge_from_wmsz_table() {
        let want = [
            Edge::Left,
            Edge::Right,
            Edge::Top,
            Edge::TopLeft,
            Edge::TopRight,
            Edge::Bottom,
            Edge::BottomLeft,
            Edge::BottomRight,
        ];
        for (i, e) in want.into_iter().enumerate() {
            assert_eq!(Edge::from_wmsz(i as u32 + 1), Some(e), "{}", i + 1);
        }
        assert_eq!(Edge::from_wmsz(0), None);
        assert_eq!(Edge::from_wmsz(9), None);
    }

    // ---- run_guarded ----

    #[test]
    fn run_guarded_returns_fallback_on_panic_and_warns_once() {
        let warned = AtomicBool::new(false);
        let log = crate::test_log::captured(|| {
            let a = run_guarded(&warned, || -> i32 { panic!("boom 1") }, || 7);
            let b = run_guarded(&warned, || -> i32 { panic!("boom 2") }, || 8);
            assert_eq!((a, b), (7, 8));
        });
        assert_eq!(
            log.matches("window aspect subclass panicked; forwarding")
                .count(),
            1,
            "{log}"
        );
    }

    #[test]
    fn run_guarded_returns_the_body_value_without_a_panic() {
        let warned = AtomicBool::new(false);
        let log = crate::test_log::captured(|| {
            let v = run_guarded(&warned, || 5, || -> i32 { panic!("fallback called") });
            assert_eq!(v, 5);
        });
        assert!(log.is_empty(), "{log}");
        assert!(!warned.load(Ordering::SeqCst));
    }

    // ---- FitOutcome ----

    #[test]
    fn fit_outcome_serializes_camel_case() {
        let rows = [
            (FitOutcome::Applied, "\"applied\""),
            (FitOutcome::AlreadyFitted, "\"alreadyFitted\""),
            (FitOutcome::Capped, "\"capped\""),
            (FitOutcome::SkippedMaximized, "\"skippedMaximized\""),
            (FitOutcome::SkippedMinimized, "\"skippedMinimized\""),
            (FitOutcome::Deferred, "\"deferred\""),
        ];
        for (outcome, want) in rows {
            assert_eq!(serde_json::to_string(&outcome).ok().as_deref(), Some(want));
        }
    }

    #[test]
    fn outcome_of_maps_every_step() {
        let plan = |capped| resize(100, 100, 1000, 654, capped);
        assert_eq!(outcome_of(&Step::NoRatio), None);
        assert_eq!(
            outcome_of(&Step::Apply(plan(false))),
            Some(FitOutcome::Applied)
        );
        assert_eq!(
            outcome_of(&Step::Apply(plan(true))),
            Some(FitOutcome::Capped)
        );
        assert_eq!(
            outcome_of(&Step::Unchanged),
            Some(FitOutcome::AlreadyFitted)
        );
        assert_eq!(
            outcome_of(&Step::Skip(FitAction::SkipMaximized)),
            Some(FitOutcome::SkippedMaximized)
        );
        assert_eq!(
            outcome_of(&Step::Skip(FitAction::SkipMinimized)),
            Some(FitOutcome::SkippedMinimized)
        );
        assert_eq!(
            outcome_of(&Step::Skip(FitAction::Defer)),
            Some(FitOutcome::Deferred)
        );
        assert_eq!(outcome_of(&Step::Skip(FitAction::Fit)), None);
    }

    // ---- drift tests ----

    #[test]
    fn config_matches_constants() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        let window = &conf["app"]["windows"][0];
        assert_eq!(window["width"].as_f64(), Some(BASE_WIDTH_PX));
        assert_eq!(window["minWidth"].as_f64(), Some(MIN_WIDTH_PX));
        assert!(window.get("minHeight").is_none());
    }

    #[test]
    fn zoom_max_matches_layout_ts() {
        let layout = include_str!("../../src/lib/layout.ts");
        assert!(layout.contains(&format!("export const ZOOM_MAX = {ZOOM_MAX};")));
    }
}
