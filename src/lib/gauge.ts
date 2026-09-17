/** Ring gauge geometry (spec §5.4): 44px box, 5px stroke, arc from 12 o'clock. */
export const RING = { size: 44, stroke: 5, radius: 19.5 } as const;

export function ringDash(pct: number | null, radius: number): { circumference: number; offset: number } {
  const circumference = 2 * Math.PI * radius;
  if (pct === null) return { circumference, offset: circumference };
  const clamped = Math.max(0, Math.min(100, pct));
  return { circumference, offset: circumference * (1 - clamped / 100) };
}
