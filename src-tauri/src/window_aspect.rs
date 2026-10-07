//! The window's content-hugging aspect lock: constants, the shared state and the pure, portable
//! decisions behind the `set_content_height` command and the Windows subclass proc.
//!
//! Everything here is physical px in `i32` unless stated, and compiles and tests on every
//! platform. No `clamp` (it panics when min > max), no `unwrap`, no `expect` outside tests: bounds
//! use ordered `min`/`max` steps, and float-to-int conversions go through an explicit
//! `floor`/`ceil`/`round`.

use crate::error::{AppError, AppResult};
use crate::platform;
use serde::Serialize;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::plugin::TauriPlugin;
use tauri::{Manager, Runtime, State, Window};

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

/// `WindowFacts` from tauri's getters, for the command and the ready hook. Conversions saturate
/// instead of panicking; `dpi` is the scale factor against the 96-dpi baseline.
pub fn facts_from_getters(
    inner: (u32, u32),
    outer_pos: (i32, i32),
    outer: (u32, u32),
    scale: f64,
    work: Option<Rect>,
    maximized: bool,
    minimized: bool,
) -> WindowFacts {
    let px = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
    let (left, top) = outer_pos;
    WindowFacts {
        client: Size {
            w: px(inner.0),
            h: px(inner.1),
        },
        outer: Rect {
            left,
            top,
            right: left.saturating_add(px(outer.0)),
            bottom: top.saturating_add(px(outer.1)),
        },
        work,
        dpi: (scale * 96.0).round() as u32,
        maximized,
        minimized,
    }
}

/// Decides one fit and applies it. A failed apply re-marks the fit pending so the next resume
/// path (size-move end, restore, DPI change, the next report) retries it.
fn run_step(
    state: &AspectState,
    facts: &WindowFacts,
    apply: impl FnOnce(&FitPlan) -> AppResult<()>,
) -> AppResult<Step> {
    let decision = step(state, facts);
    if let Step::Apply(plan) = &decision {
        if let Err(e) = apply(plan) {
            state.mark_pending();
            return Err(e);
        }
    }
    Ok(decision)
}

/// The command's whole decision, with the window reads and the apply passed in so it runs
/// without a window. A rejected report returns before `facts` is called; once accepted the ratio
/// stays stored whatever happens next.
pub fn handle_report(
    state: &AspectState,
    content_h: f64,
    facts: impl FnOnce() -> AppResult<WindowFacts>,
    apply: impl FnOnce(&FitPlan) -> AppResult<()>,
) -> AppResult<(FitOutcome, Option<FitPlan>)> {
    accept_report(state, content_h)?;
    let facts = facts()?;
    let decision = run_step(state, &facts, apply)?;
    let outcome = outcome_of(&decision)
        .ok_or_else(|| AppError::Internal("ratio missing after report".into()))?;
    let plan = match decision {
        Step::Apply(plan) => Some(plan),
        _ => None,
    };
    Ok((outcome, plan))
}

/// The camelCase name the frontend sees; a test ties it to the serde output.
fn outcome_name(outcome: FitOutcome) -> &'static str {
    match outcome {
        FitOutcome::Applied => "applied",
        FitOutcome::AlreadyFitted => "alreadyFitted",
        FitOutcome::Capped => "capped",
        FitOutcome::SkippedMaximized => "skippedMaximized",
        FitOutcome::SkippedMinimized => "skippedMinimized",
        FitOutcome::Deferred => "deferred",
    }
}

/// The one "window fitted" line, for every source (report, ready, restored, dpi).
pub(crate) fn log_fitted(label: &str, source: &str, plan: &FitPlan, state: &AspectState) {
    if let FitPlan::Resize {
        outer,
        client_w,
        client_h,
        capped,
    } = plan
    {
        let ratio = state.ratio().unwrap_or(0.0);
        tracing::info!(
            label,
            source,
            client_w,
            client_h,
            top = outer.top,
            capped,
            ratio = format!("{ratio:.6}"),
            "window fitted"
        );
    }
}

fn getter_error(label: &str, getter: &str, e: tauri::Error) -> AppError {
    tracing::warn!(label, error = %e, "window facts unavailable");
    AppError::Internal(format!("window {getter} unavailable: {e}"))
}

/// The window's facts from tauri's getters. This is the one work-area owner outside the proc.
fn window_facts<R: Runtime>(window: &Window<R>) -> AppResult<WindowFacts> {
    let label = window.label();
    let maximized = window
        .is_maximized()
        .map_err(|e| getter_error(label, "maximized state", e))?;
    let minimized = window
        .is_minimized()
        .map_err(|e| getter_error(label, "minimized state", e))?;
    let inner = window
        .inner_size()
        .map_err(|e| getter_error(label, "inner size", e))?;
    let pos = window
        .outer_position()
        .map_err(|e| getter_error(label, "position", e))?;
    let outer = window
        .outer_size()
        .map_err(|e| getter_error(label, "outer size", e))?;
    let scale = window
        .scale_factor()
        .map_err(|e| getter_error(label, "scale factor", e))?;
    let monitor = window
        .current_monitor()
        .map_err(|e| getter_error(label, "monitor", e))?;
    let work = monitor.map(|m| {
        let area = m.work_area();
        let px = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
        Rect {
            left: area.position.x,
            top: area.position.y,
            right: area.position.x.saturating_add(px(area.size.width)),
            bottom: area.position.y.saturating_add(px(area.size.height)),
        }
    });
    Ok(facts_from_getters(
        (inner.width, inner.height),
        (pos.x, pos.y),
        (outer.width, outer.height),
        scale,
        work,
        maximized,
        minimized,
    ))
}

fn log_report(
    label: &str,
    state: &AspectState,
    content_h: f64,
    result: &AppResult<(FitOutcome, Option<FitPlan>)>,
) {
    let ratio = format!("{:.6}", state.ratio().unwrap_or(0.0));
    match result {
        Ok((outcome, plan)) => {
            tracing::debug!(
                label,
                content_h,
                ratio,
                outcome = outcome_name(*outcome),
                "content height report"
            );
            if let Some(plan) = plan {
                log_fitted(label, "report", plan, state);
            }
            match outcome {
                FitOutcome::SkippedMaximized => {
                    tracing::info!(label, reason = "maximized", pending = true, "fit skipped")
                }
                FitOutcome::SkippedMinimized => {
                    tracing::info!(label, reason = "minimized", pending = true, "fit skipped")
                }
                FitOutcome::Deferred => tracing::debug!(label, "fit deferred"),
                FitOutcome::Applied | FitOutcome::AlreadyFitted | FitOutcome::Capped => {}
            }
        }
        Err(AppError::OutOfRange(_)) => {
            tracing::warn!(label, content_h, "content height rejected");
        }
        Err(e) => tracing::debug!(
            label,
            content_h,
            ratio,
            outcome = "error",
            error = %e,
            "content height report"
        ),
    }
}

/// The frontend's measured content height: stores the ratio, then fits the window to it now.
/// Runs on the main thread (a sync command), the thread that owns the window.
#[tauri::command]
pub fn set_content_height(
    window: tauri::Window,
    state: State<'_, Arc<AspectState>>,
    content_h: f64,
) -> AppResult<FitOutcome> {
    let result = handle_report(
        &state,
        content_h,
        || window_facts(&window),
        |plan| platform::apply_fit(&window, plan),
    );
    log_report(window.label(), &state, content_h, &result);
    result.map(|(outcome, _)| outcome)
}

/// Fits a window from the stored ratio without a report: a window rebuilt after close-to-tray
/// is sized right before the frontend measures again.
pub fn fit_now<R: Runtime>(window: &Window<R>, state: &AspectState, source: &str) {
    let label = window.label();
    let Ok(facts) = window_facts(window) else {
        return;
    };
    match run_step(state, &facts, |plan| platform::apply_fit(window, plan)) {
        Ok(Step::NoRatio) => tracing::debug!(label, "fit skipped: ratio unknown"),
        Ok(Step::Apply(plan)) => log_fitted(label, source, &plan, state),
        Ok(Step::Unchanged | Step::Skip(_)) => {}
        Err(e) => tracing::warn!(label, error = %e, "window fit failed"),
    }
}

/// Registers the shared state and, for every window, the subclass and a first fit. Must be
/// registered before any window exists, which `tauri::Builder` guarantees for a plugin.
pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::<R>::new("window-aspect")
        .setup(|app, _api| {
            app.manage(Arc::new(AspectState::default()));
            Ok(())
        })
        .on_window_ready(|window| {
            let label = window.label().to_string();
            let Some(state) = window.try_state::<Arc<AspectState>>() else {
                tracing::warn!(label, "window aspect state missing");
                return;
            };
            let state = Arc::clone(state.inner());
            platform::install(&window, Arc::clone(&state));
            fit_now(&window, &state, "ready");
        })
        .build()
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

    #[test]
    fn fit_outcomes_match_fit_ts() {
        let ts = include_str!("../../src/lib/fit.ts");
        let line = ts
            .lines()
            .find(|l| l.starts_with("export const FIT_OUTCOMES"))
            .expect("FIT_OUTCOMES line in fit.ts");
        let list = line
            .split_once('[')
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(inside, _)| inside)
            .expect("bracketed list");
        let ts_names: Vec<&str> = list
            .split(',')
            .map(|s| s.trim().trim_matches('"'))
            .collect();
        let rust_names: Vec<String> = [
            FitOutcome::Applied,
            FitOutcome::AlreadyFitted,
            FitOutcome::Capped,
            FitOutcome::SkippedMaximized,
            FitOutcome::SkippedMinimized,
            FitOutcome::Deferred,
        ]
        .iter()
        .map(|o| {
            serde_json::to_string(o)
                .expect("serialize")
                .trim_matches('"')
                .to_string()
        })
        .collect();
        assert_eq!(ts_names, rust_names);
    }

    // ---- Task 7: the command glue ----

    #[test]
    fn facts_from_getters_rounds_dpi() {
        let f = facts_from_getters((1000, 654), (10, 20), (1016, 693), 1.5, None, false, false);
        assert_eq!(f.dpi, 144);
        assert_eq!(f.client, Size { w: 1000, h: 654 });
        assert_eq!(f.outer, rect(10, 20, 1026, 713));
        let g = facts_from_getters((1000, 654), (10, 20), (1016, 693), 1.25, None, true, false);
        assert_eq!(g.dpi, 120);
        assert!(g.maximized && !g.minimized);
    }

    #[test]
    fn facts_from_getters_without_a_monitor_has_no_work_area() {
        let f = facts_from_getters((1, 1), (0, 0), (1, 1), 1.0, None, false, false);
        assert_eq!(f.work, None);
        let g = facts_from_getters((1, 1), (0, 0), (1, 1), 1.0, Some(WORK), false, true);
        assert_eq!(g.work, Some(WORK));
        assert!(g.minimized);
    }

    #[test]
    fn facts_from_getters_saturates_without_panicking() {
        let f = facts_from_getters(
            (u32::MAX, u32::MAX),
            (i32::MAX, i32::MAX),
            (u32::MAX, u32::MAX),
            1.0,
            None,
            false,
            false,
        );
        assert_eq!(
            f.client,
            Size {
                w: i32::MAX,
                h: i32::MAX
            }
        );
        assert_eq!(f.outer, rect(i32::MAX, i32::MAX, i32::MAX, i32::MAX));
    }

    #[test]
    fn handle_report_rejects_without_reading_facts() {
        let state = AspectState::default();
        let r = handle_report(
            &state,
            0.0,
            || panic!("facts must not be read for a rejected report"),
            |_| panic!("nothing is applied for a rejected report"),
        );
        assert!(out_of_range(r));
        assert_eq!(state.ratio(), None);
    }

    #[test]
    fn handle_report_maximized_skips_and_marks_pending() {
        let state = AspectState::default();
        let f = WindowFacts {
            maximized: true,
            ..facts(1000, 700, 0)
        };
        let r = handle_report(
            &state,
            640.0,
            || Ok(f),
            |_| panic!("a skipped fit applies nothing"),
        );
        assert!(matches!(r, Ok((FitOutcome::SkippedMaximized, None))));
        assert!(state.is_pending());
        assert_eq!(state.ratio(), Some(RATIO));
    }

    #[test]
    fn handle_report_in_a_size_move_is_deferred() {
        let state = AspectState::default();
        state.begin_size_move();
        let r = handle_report(
            &state,
            640.0,
            || Ok(facts(1000, 700, 0)),
            |_| panic!("a deferred fit applies nothing"),
        );
        assert!(matches!(r, Ok((FitOutcome::Deferred, None))));
        assert!(state.is_pending());
    }

    #[test]
    fn handle_report_fitted_window_is_already_fitted() {
        let state = AspectState::default();
        let r = handle_report(
            &state,
            640.0,
            || Ok(facts(1000, 654, 0)),
            |_| panic!("a fitted window applies nothing"),
        );
        assert!(matches!(r, Ok((FitOutcome::AlreadyFitted, None))));
        assert!(!state.is_pending());
    }

    #[test]
    fn handle_report_off_shape_applies() {
        let state = AspectState::default();
        let seen = std::cell::Cell::new(None);
        let r = handle_report(
            &state,
            640.0,
            || Ok(facts(1000, 700, 0)),
            |plan| {
                seen.set(Some(*plan));
                Ok(())
            },
        );
        let want = resize(100, 0, 1000, 654, false);
        assert!(matches!(r, Ok((FitOutcome::Applied, Some(p))) if p == want));
        assert_eq!(seen.get(), Some(want));
        assert!(!state.is_pending());
    }

    #[test]
    fn handle_report_facts_error_keeps_the_ratio() {
        let state = AspectState::default();
        let r = handle_report(
            &state,
            640.0,
            || Err(AppError::Internal("no facts".into())),
            |_| panic!("nothing is applied without facts"),
        );
        assert!(matches!(r, Err(AppError::Internal(m)) if m == "no facts"));
        assert_eq!(state.ratio(), Some(RATIO));
    }

    #[test]
    fn handle_report_apply_failure_errs_and_keeps_pending() {
        let state = AspectState::default();
        let r = handle_report(
            &state,
            640.0,
            || Ok(facts(1000, 700, 0)),
            |_| Err(AppError::Internal("apply failed".into())),
        );
        assert!(matches!(r, Err(AppError::Internal(m)) if m == "apply failed"));
        assert!(state.is_pending());
        assert_eq!(state.ratio(), Some(RATIO));
    }

    #[test]
    fn outcome_name_matches_the_serialized_name() {
        for outcome in [
            FitOutcome::Applied,
            FitOutcome::AlreadyFitted,
            FitOutcome::Capped,
            FitOutcome::SkippedMaximized,
            FitOutcome::SkippedMinimized,
            FitOutcome::Deferred,
        ] {
            let json = serde_json::to_string(&outcome).expect("serializes");
            assert_eq!(json, format!("\"{}\"", outcome_name(outcome)));
        }
    }
}
