import type { CSSProperties } from "react";
import { DEFAULT_ORDER, GRID_GAP, gridMinWidth, visibleColumns } from "./columns";
import type { ColumnKey } from "./columns";
import { toLocal } from "./drag";

export type Layout = "full" | "narrow" | "cards";

/**
 * Every shared length lives here (local, unzoomed px) and reaches styles.css
 * through the custom properties `shellVars` returns. styles.css only reads
 * them; css-contract.test.ts fails on a literal where a variable belongs.
 */
/** `.app` padding and the gap between the shell's stacked blocks. */
export const APP_GUTTER = 6;
/** `.row-grid` height. */
export const ROW_HEIGHT = 40;
/** `.row` bottom border width (chartHeightPx adds it to ROW_HEIGHT). */
export const ROW_BORDER = 1;
/** `.row-grid` and `.thead` horizontal padding. */
export const ROW_PAD_X = 8;
/** `.panel` border width. */
export const PANEL_BORDER = 1;
/** Bounds of the card-layout ring (read by `.ring`). */
export const RING_MIN_PX = 36;
export const RING_MAX_PX = 120;

/** Columns the narrow layout hides on top of the user's own hidden set. */
const NARROW_HIDDEN: readonly ColumnKey[] = ["updated", "model"];

/**
 * Horizontal px around the grid tracks that the table cannot use:
 * `.app` padding 2×APP_GUTTER + `.panel` border 2×PANEL_BORDER + `.row-grid`
 * padding 2×ROW_PAD_X + 17 for WebView2's classic vertical scrollbar
 * (window.innerWidth includes it).
 */
export const SHELL_PADDING = 2 * APP_GUTTER + 2 * PANEL_BORDER + 2 * ROW_PAD_X + 17;

/**
 * Local (unzoomed) px at which the table stops fitting. Computed, never typed:
 * a hand literal drifts from the CSS. NARROW_HIDDEN must be declared above
 * this line (a `const` read before its declaration is a TDZ error).
 */
export const BREAKPOINTS: { readonly narrow: number; readonly cards: number } = {
  narrow: gridMinWidth(DEFAULT_ORDER) + SHELL_PADDING,
  cards: gridMinWidth(visibleColumns(DEFAULT_ORDER, NARROW_HIDDEN)) + SHELL_PADDING,
};

/** The CSS custom properties the shell sets for styles.css to read. */
export function shellVars(): Record<string, string> {
  return {
    "--gutter": `${APP_GUTTER}px`,
    "--row-h": `${ROW_HEIGHT}px`,
    "--row-border": `${ROW_BORDER}px`,
    "--row-pad-x": `${ROW_PAD_X}px`,
    "--grid-gap": `${GRID_GAP}px`,
    "--panel-border": `${PANEL_BORDER}px`,
    "--ring-min": `${RING_MIN_PX}px`,
    "--ring-max": `${RING_MAX_PX}px`,
  };
}

/** The shell's inline style: the text-size zoom plus every shell variable. */
export function shellStyle(zoom: number): CSSProperties {
  // `zoom` and the custom properties are valid inline styles that
  // CSSProperties does not list; this is the one typed cast.
  return { zoom, ...shellVars() } as CSSProperties;
}

export function layoutFor(viewportWidthPx: number, zoom: number): Layout {
  const w = toLocal(viewportWidthPx, zoom);
  if (w < BREAKPOINTS.cards) return "cards";
  if (w < BREAKPOINTS.narrow) return "narrow";
  return "full";
}

/** Columns the layout hides on top of the user's own hidden set. */
export function autoHiddenColumns(layout: Layout): readonly ColumnKey[] {
  return layout === "narrow" ? NARROW_HIDDEN : [];
}
