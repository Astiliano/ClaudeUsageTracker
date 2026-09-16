/** Data colours from docs/design/usage-tracker-kit.js (THEME). */
export const THEME = {
  ok: "#7aa2f7",
  warn: "#d8a94f",
  crit: "#e0705f",
  live: "#5fb98a",
  idle: "#4b5359",
  meta: "#8b949e",
  metaWarn: "#d08c80",
} as const;

export const THRESHOLDS = { warn: 70, crit: 95 } as const;

export type MetricTone = "ok" | "warn" | "crit";

export function metricTone(pct: number): MetricTone {
  if (pct >= THRESHOLDS.crit) return "crit";
  if (pct >= THRESHOLDS.warn) return "warn";
  return "ok";
}

export function metricColor(pct: number): string {
  return THEME[metricTone(pct)];
}

export type FontKey = "system" | "plex" | "jetbrains";
export interface FontChoice { label: string; ui: string; mono: string }

export const FONTS: Record<FontKey, FontChoice> = {
  system: {
    label: "System",
    ui: "ui-sans-serif, 'Segoe UI', Helvetica, Arial, sans-serif",
    mono: "ui-monospace, 'Cascadia Mono', Consolas, 'SF Mono', monospace",
  },
  plex: {
    label: "IBM Plex",
    ui: "'IBM Plex Sans', Helvetica, sans-serif",
    mono: "'IBM Plex Mono', monospace",
  },
  jetbrains: {
    label: "JetBrains",
    ui: "ui-sans-serif, 'Segoe UI', Helvetica, sans-serif",
    mono: "'JetBrains Mono', monospace",
  },
};
export const FONT_KEYS: readonly FontKey[] = ["system", "plex", "jetbrains"];
export function isFontKey(v: unknown): v is FontKey {
  return typeof v === "string" && (FONT_KEYS as readonly string[]).includes(v);
}

export type SizeKey = "sm" | "md" | "lg" | "xl";
export interface SizeChoice { label: string; zoom: number }
export const SIZES: Record<SizeKey, SizeChoice> = {
  sm: { label: "small", zoom: 0.92 },
  md: { label: "default", zoom: 1 },
  lg: { label: "large", zoom: 1.1 },
  xl: { label: "largest", zoom: 1.22 },
};
export const SIZE_KEYS: readonly SizeKey[] = ["sm", "md", "lg", "xl"];
export function isSizeKey(v: unknown): v is SizeKey {
  return typeof v === "string" && (SIZE_KEYS as readonly string[]).includes(v);
}
