/** Pointer-drag geometry for row and column reordering (pure, no DOM). */

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

/**
 * Converts a screen-space (client) pixel quantity into the element's local,
 * unzoomed CSS px, given the active CSS `zoom` factor. A zero or non-finite
 * zoom is treated as 1 (no scaling) rather than dividing by zero or NaN.
 */
export function toLocal(screenPx: number, zoom: number): number {
  const z = Number.isFinite(zoom) && zoom > 0 ? zoom : 1;
  return screenPx / z;
}

export function rowDragTarget(index: number, dy: number, rowH: number, length: number): number {
  return clamp(index + Math.round(dy / rowH), 0, Math.max(0, length - 1));
}

export function rowShift(i: number, dragIndex: number, target: number, rowH: number): number {
  if (i > dragIndex && i <= target) return -rowH;
  if (i < dragIndex && i >= target) return rowH;
  return 0;
}

export interface Rect { left: number; right: number; width: number }

export function colDragTarget(rects: readonly Rect[], clientX: number): number {
  if (rects.length === 0) return 0;
  const idx = rects.findIndex((r) => clientX < r.left + r.width / 2);
  return idx < 0 ? rects.length - 1 : idx;
}

export function colLineX(rects: readonly Rect[], target: number, index: number, wrapLeft: number): number {
  const r = rects[target];
  if (r === undefined) return 0;
  return (target <= index ? r.left - 5 : r.right + 5) - wrapLeft;
}

/**
 * Decides what to do, at drag end, with a rows update that arrived mid-drag
 * and was stashed rather than applied immediately: pass it through unless a
 * refetch is about to resync `order` on its own, in which case discard it
 * (re-applying a pre-refetch snapshot afterward would revert the fresher
 * state). `refetchWillResync` is not the same as "did this drag commit" —
 * a committed row reorder triggers a refetch, but a committed column
 * reorder never does (it only writes local prefs), so the caller passes
 * `false` for every column drag regardless of whether it committed.
 */
export function settlePendingRows<T>(pending: T[] | null, refetchWillResync: boolean): T[] | null {
  return refetchWillResync ? null : pending;
}
