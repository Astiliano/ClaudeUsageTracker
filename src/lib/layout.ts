import type { CSSProperties } from "react";
import { GRID_GAP } from "./columns";
import { toLocal } from "./drag";

/**
 * Every shared length lives here (local, unzoomed px) and reaches styles.css
 * through the custom properties `shellVars` returns. styles.css only reads
 * them; css-contract.test.ts fails on a literal where a variable belongs.
 */
/** `.app` padding and the gap between the shell's stacked blocks. */
export const APP_GUTTER = 6;
/** `.row-grid` height. */
export const ROW_HEIGHT = 40;
/** `.row` bottom border width (the reorder drag stride adds it to ROW_HEIGHT). */
export const ROW_BORDER = 1;
/** `.row-grid` and `.thead` horizontal padding. */
export const ROW_PAD_X = 8;
/** `.panel` border width. */
export const PANEL_BORDER = 1;
/** Local px height of the drawer's history chart (`.chart`); independent of the window, so the content height never depends on it. */
export const CHART_HEIGHT = 220;
/** The CSS custom properties the shell sets for styles.css to read. */
export function shellVars(): Record<string, string> {
  return {
    "--gutter": `${APP_GUTTER}px`,
    "--row-h": `${ROW_HEIGHT}px`,
    "--row-border": `${ROW_BORDER}px`,
    "--row-pad-x": `${ROW_PAD_X}px`,
    "--grid-gap": `${GRID_GAP}px`,
    "--panel-border": `${PANEL_BORDER}px`,
    "--base-w": `${BASE_WIDTH}px`,
    "--chart-h": `${CHART_HEIGHT}px`,
  };
}

/**
 * The shell's inline style: the window zoom, every shell variable, and
 * `--viewport-h`, the window height in local px. styles.css never uses `vh`:
 * Chromium multiplies viewport units by CSS zoom, so `100vh` under a zoom
 * of 1.1 is 110% of the window and the page scrolls.
 */
export function shellStyle(zoom: number, viewportHeightPx: number): CSSProperties {
  const h = toLocal(viewportHeightPx, zoom);
  const viewportH = Number.isFinite(h) && h > 0 ? h : 0;
  // `zoom` and the custom properties are valid inline styles that
  // CSSProperties does not list; this is the one typed cast.
  return { zoom, ...shellVars(), "--viewport-h": `${viewportH}px` } as CSSProperties;
}

/**
 * The canvas the UI is designed for: the window configured in
 * src-tauri/tauri.conf.json (layout.test.ts fails if the two drift).
 */
export const BASE_WIDTH = 980;
/** Bounds of the window-derived zoom. */
export const ZOOM_MIN = 0.75;
export const ZOOM_MAX = 2.5;

/**
 * The CSS zoom for a window of the given size: `min(w / BASE_WIDTH, h / contentH)`,
 * clamped to [ZOOM_MIN, ZOOM_MAX]. `contentH` is the content's height in local
 * px; a null, non-finite or non-positive one leaves the width term alone. A
 * non-finite or non-positive window dimension (jsdom reports 0 before mount)
 * gives 1: a zoom of 0 or NaN would blank the page.
 */
export function windowZoom(widthPx: number, heightPx: number, contentH: number | null): number {
  if (!Number.isFinite(widthPx) || !Number.isFinite(heightPx) || widthPx <= 0 || heightPx <= 0) {
    return 1;
  }
  const widthTerm = widthPx / BASE_WIDTH;
  const usable = contentH !== null && Number.isFinite(contentH) && contentH > 0;
  const fit = usable ? Math.min(widthTerm, heightPx / contentH) : widthTerm;
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, fit));
}
