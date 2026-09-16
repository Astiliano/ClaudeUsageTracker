/** Pointer-drag geometry for row and column reordering (pure, no DOM). */

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
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
