const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/**
 * "resets in 3h 12m", clamped at "resets now" when the instant has passed,
 * and an em dash when the CLI omitted the reset clause.
 */
export function formatCountdown(resetsAt: number | null, now: number): string {
  if (resetsAt === null) {
    return "—";
  }
  const remaining = resetsAt - now;
  if (remaining <= 0) {
    return "resets now";
  }
  if (remaining < MINUTE) {
    return "resets in <1m";
  }
  if (remaining < HOUR) {
    return `resets in ${Math.floor(remaining / MINUTE)}m`;
  }
  if (remaining < DAY) {
    const hours = Math.floor(remaining / HOUR);
    const minutes = Math.floor((remaining % HOUR) / MINUTE);
    return `resets in ${hours}h ${minutes}m`;
  }
  const days = Math.floor(remaining / DAY);
  const hours = Math.floor((remaining % DAY) / HOUR);
  return `resets in ${days}d ${hours}h`;
}

/** "42 s ago", ticking live from a 1 s interval in the caller. */
export function formatAgo(takenAt: number | null, now: number): string {
  if (takenAt === null) {
    return "never";
  }
  const elapsed = now - takenAt;
  if (elapsed < 0) {
    return "just now";
  }
  if (elapsed < MINUTE) {
    return `${Math.floor(elapsed / SECOND)} s ago`;
  }
  if (elapsed < HOUR) {
    return `${Math.floor(elapsed / MINUTE)} min ago`;
  }
  if (elapsed < DAY) {
    return `${Math.floor(elapsed / HOUR)} h ago`;
  }
  return `${Math.floor(elapsed / DAY)} d ago`;
}
