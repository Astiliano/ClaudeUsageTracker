//! Non-Windows window-platform code: no live aspect lock, a portable `apply_fit`.

use crate::error::{AppError, AppResult};
use crate::window_aspect::FitPlan;
use tauri::{PhysicalPosition, PhysicalSize, Runtime, Window};

/// Applies a fit through Tauri's setters: `set_position` only when the top changes, then
/// `set_size`, which sets the inner (client) size.
pub fn apply_fit<R: Runtime>(window: &Window<R>, plan: &FitPlan) -> AppResult<()> {
    let FitPlan::Resize {
        outer,
        client_w,
        client_h,
        ..
    } = plan
    else {
        return Ok(());
    };
    let current = window
        .outer_position()
        .map_err(|e| AppError::Internal(format!("window position unavailable: {e}")))?;
    if current.y != outer.top {
        window
            .set_position(PhysicalPosition::new(outer.left, outer.top))
            .map_err(|e| AppError::Internal(format!("set_position failed: {e}")))?;
    }
    let (w, h) = (
        u32::try_from(*client_w).unwrap_or(0),
        u32::try_from(*client_h).unwrap_or(0),
    );
    window
        .set_size(PhysicalSize::new(w, h))
        .map_err(|e| AppError::Internal(format!("set_size failed: {e}")))
}
