/** Ring gauge geometry (spec §5.4): 44px box, 5px stroke, arc from 12 o'clock. */
export const RING = { size: 44, stroke: 5, radius: 19.5 } as const;

/** The two ring sizes: 44px in the cards, 20px on the system line. */
export const RING_SIZES = {
  md: RING,
  sm: { size: 20, stroke: 3, radius: 8.5 },
} as const;

export function ringDash(pct: number | null, radius: number): { circumference: number; offset: number } {
  const circumference = 2 * Math.PI * radius;
  if (pct === null) return { circumference, offset: circumference };
  const clamped = Math.max(0, Math.min(100, pct));
  return { circumference, offset: circumference * (1 - clamped / 100) };
}
