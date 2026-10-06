import { ROW_BORDER, ROW_HEIGHT } from "./layout";

/** Chart height bounds, local (unzoomed) px. */
export const CHART_MIN_PX = 72;
/** Binds only on very tall screens. */
export const CHART_MAX_PX = 1200;

export interface ChartFit {
  /** `window.innerHeight`, viewport (post-zoom) px. */
  viewportPx: number;
  /** Drawer height minus chart height, viewport (post-zoom) px. */
  chromePx: number;
  /** The Text size zoom factor on the shell. */
  zoom: number;
}

/**
 * Local px height that makes row + drawer equal the viewport, clamped. The
 * row is ROW_HEIGHT local px plus its ROW_BORDER bottom border; the viewport and
 * chrome are post-zoom px, so both divide by zoom.
 */
export function chartHeightPx(f: ChartFit): number {
  const fit = Math.floor(f.viewportPx / f.zoom - (ROW_HEIGHT + ROW_BORDER) - f.chromePx / f.zoom);
  return Math.min(CHART_MAX_PX, Math.max(CHART_MIN_PX, fit));
}
