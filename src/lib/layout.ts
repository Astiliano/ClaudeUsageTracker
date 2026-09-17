import type { ColumnKey } from "./columns";
import { toLocal } from "./drag";

export type Layout = "full" | "narrow" | "cards";

/** Local (unzoomed) px. See spec §5.2 for the arithmetic behind each number. */
export const BREAKPOINTS = { narrow: 820, cards: 640 } as const;

/**
 * Horizontal px around the grid tracks that the table cannot use:
 * `.app` padding 2×22 + `.panel` border 2×1 + `.row-grid` padding 2×16
 * + 17 for WebView2's classic vertical scrollbar (window.innerWidth
 * includes it). Keep the three CSS terms in step with styles.css.
 */
export const SHELL_PADDING = 95;

export function layoutFor(viewportWidthPx: number, zoom: number): Layout {
  const w = toLocal(viewportWidthPx, zoom);
  if (w < BREAKPOINTS.cards) return "cards";
  if (w < BREAKPOINTS.narrow) return "narrow";
  return "full";
}

const NARROW_HIDDEN: readonly ColumnKey[] = ["updated", "model"];

/** Columns the layout hides on top of the user's own hidden set. */
export function autoHiddenColumns(layout: Layout): readonly ColumnKey[] {
  return layout === "narrow" ? NARROW_HIDDEN : [];
}
