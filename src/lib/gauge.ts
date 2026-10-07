/** Ring gauge geometry, arc from 12 o'clock: 44px in the cards view, 20px glyph on the system line. */
export const RING_SIZES = {
  md: { size: 44, stroke: 5, radius: 19.5 },
  sm: { size: 20, stroke: 3, radius: 8.5 },
} as const;

/** SVG viewBox for a ring geometry: the box in geometry units, so the stroke scales with the element. */
export function ringViewBox(geometry: { readonly size: number }): string {
  return `0 0 ${geometry.size} ${geometry.size}`;
}

export function ringDash(pct: number | null, radius: number): { circumference: number; offset: number } {
  const circumference = 2 * Math.PI * radius;
  if (pct === null) return { circumference, offset: circumference };
  const clamped = Math.max(0, Math.min(100, pct));
  return { circumference, offset: circumference * (1 - clamped / 100) };
}
