import { bannerFor } from "./banner";
import { formatCountdown } from "./format";
import type { Pill } from "./pill";
import { THEME, THRESHOLDS } from "./theme";
import type { AccountRow, Dashboard, ModelWindow, Win } from "./types";

/** Account dot: disabled > never polled > fault tone > weekly limit > live. */
export function accountDotColor(row: AccountRow, pill: Pill): string {
  if (!row.account.enabled || pill.kind === "pending") return THEME.idle;
  if (pill.tone === "error") return THEME.crit;
  if (pill.tone === "warn") return THEME.warn;
  const week = row.latest?.week_all?.pct ?? 0;
  return week >= THRESHOLDS.crit ? THEME.crit : THEME.live;
}

export interface ModelSummary { pct: number; note: string; title: string }

export function summarizeModels(models: readonly ModelWindow[]): ModelSummary | null {
  if (models.length === 0) return null;
  let top = models[0];
  for (const m of models) if (m.pct > top.pct) top = m;
  const rest = models.length - 1;
  return {
    pct: top.pct,
    note: rest > 0 ? `${top.label} · +${rest}` : top.label,
    title: models.map((m) => `${m.label} ${m.pct}%`).join(" · "),
  };
}

export function weekNote(pct: number): { text: string; warn: boolean } {
  return pct >= THRESHOLDS.crit ? { text: "at limit", warn: true } : { text: "all models", warn: false };
}

export function sessionNote(session: Win | null, now: number): string {
  if (session === null) return "no data";
  if (session.resets_at === null) return "idle";
  return formatCountdown(session.resets_at, now);
}

export function accountCountLabel(n: number): string {
  return `${n} ${n === 1 ? "account" : "accounts"}`;
}

export interface Chip { dot: "live" | "idle" | "warn" | "crit"; text: string }

export function chipFor(dashboard: Dashboard): Chip {
  const banner = bannerFor(dashboard);
  switch (banner?.kind) {
    case "halted": return { dot: "crit", text: "polling halted" };
    case "stalled": return { dot: "warn", text: "stalled, recovered" };
    case "no_binary": return { dot: "warn", text: "no claude binary" };
    case "no_accounts": return { dot: "warn", text: "no enabled accounts" };
    case "active": return { dot: "live", text: `polling every ${dashboard.interval_secs} s` };
    default: return { dot: "idle", text: "idle · waits for Claude Code" };
  }
}
