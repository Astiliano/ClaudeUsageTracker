import { bannerFor } from "./banner";
import type { BannerKind } from "./banner";
import { formatLeft } from "./format";
import type { Pill } from "./pill";
import { processCountSuffix } from "./system";
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

export interface ModelSummary { pct: number; label: string; note: string; title: string }

export function summarizeModels(models: readonly ModelWindow[]): ModelSummary | null {
  if (models.length === 0) return null;
  let top = models[0];
  for (const m of models) if (m.pct > top.pct) top = m;
  const rest = models.length - 1;
  return {
    pct: top.pct,
    label: top.label,
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
  return formatLeft(session.resets_at, now);
}

export function accountCountLabel(n: number): string {
  return `${n} ${n === 1 ? "account" : "accounts"}`;
}

export interface Chip { dot: "live" | "idle" | "warn" | "crit"; text: string }

export function chipFor(dashboard: Dashboard, claudeProcesses: number | null = null): Chip {
  const banner = bannerFor(dashboard);
  switch (banner?.kind) {
    case "halted": return { dot: "crit", text: "polling halted" };
    case "stalled": return { dot: "warn", text: "stalled, recovered" };
    case "no_binary": return { dot: "warn", text: "no claude binary" };
    case "no_accounts": return { dot: "warn", text: "no enabled accounts" };
    // Only these two chips carry the count, so only these two compute it.
    case "active": return {
      dot: "live",
      text: `polling every ${dashboard.interval_secs} s${processCountSuffix(claudeProcesses)}`,
    };
    default: return {
      dot: "idle",
      text: `idle · waits for Claude Code${processCountSuffix(claudeProcesses)}`,
    };
  }
}

/**
 * Where the Claude process count goes, so it is shown exactly once. The chip
 * only carries it when it has room (not cards) and its text is about polling
 * at all; every other chip leaves it to the system line, so a halted or
 * stalled header still says how many Claude processes exist.
 */
export function countPlacement(kind: BannerKind, compact: boolean): "chip" | "line" {
  if (compact) return "line";
  return kind === "active" || kind === "idle" ? "chip" : "line";
}
