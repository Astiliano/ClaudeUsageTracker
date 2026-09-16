import type { Dashboard } from "./types";

export type BannerKind =
  | "halted"
  | "stalled"
  | "no_binary"
  | "no_accounts"
  | "active"
  | "idle";

export interface Banner {
  kind: BannerKind;
  text: string;
  action?: "clear_halt" | "open_settings";
  tone: "error" | "warn" | "info";
}

function clockOf(epochMs: number): string {
  const d = new Date(epochMs);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

/**
 * Header state, first match wins. The no-enabled-accounts case is a purely
 * presentational derivation over the returned rows, not a usage computation.
 */
export function bannerFor(dashboard: Dashboard): Banner | null {
  if (dashboard.halted !== null) {
    return {
      kind: "halted",
      tone: "error",
      action: "clear_halt",
      text:
        "Polling halted: a /usage call reached the model (see log). " +
        "Clear only after confirming the Claude Code version/flags",
    };
  }

  if (dashboard.stalled_at !== null) {
    return {
      kind: "stalled",
      tone: "warn",
      text: `Poller stalled at ${clockOf(dashboard.stalled_at)} — recovered`,
    };
  }

  if (dashboard.binary.path === null) {
    return {
      kind: "no_binary",
      tone: "warn",
      action: "open_settings",
      text: "Claude binary not found — set it in Settings",
    };
  }

  const anyEnabled = dashboard.accounts.some((r) => r.account.enabled);
  if (!anyEnabled) {
    return {
      kind: "no_accounts",
      tone: "warn",
      text: "Polling paused: no enabled accounts",
    };
  }

  if (dashboard.gate === "active") {
    return {
      kind: "active",
      tone: "info",
      text: `Claude running — polling ${dashboard.interval_secs} s after each cycle`,
    };
  }

  return {
    kind: "idle",
    tone: "info",
    text: "Idle — will resume when Claude Code starts",
  };
}
