import { describe, expect, it } from "vitest";
import { bannerFor } from "./banner";
import { formatBytes } from "./system";
import type { Account, AccountRow, Dashboard } from "./types";

function account(over: Partial<Account> = {}): Account {
  return {
    id: "a1",
    label: "claude3",
    config_dir: "/home/josh/.claude3",
    enabled: true,
    disabled_reason: null,
    created_at: 0,
    sort_order: 0,
    ...over,
  };
}

function row(over: Partial<AccountRow> = {}): AccountRow {
  return { account: account(), latest: null, backoff_until: null, ...over };
}

function dash(over: Partial<Dashboard> = {}): Dashboard {
  return {
    accounts: [row()],
    gate: "idle",
    busy: false,
    halted: null,
    stalled_at: null,
    binary: { path: "/home/josh/.local/bin/claude", source: "local_bin" },
    interval_secs: 60,
    memory_hold: null,
    ...over,
  };
}

const MIB = 1024 * 1024;
const HOLD = { available_bytes: 512 * MIB, floor_bytes: 1536 * MIB, since: 0 };

describe("bannerFor", () => {
  it("shows the halt banner ahead of everything else", () => {
    const b = bannerFor(
      dash({
        halted: "guard_tripped:1700000000000",
        stalled_at: 1,
        binary: { path: null, source: null },
        accounts: [row({ account: account({ enabled: false }) })],
      }),
    );
    expect(b?.kind).toBe("halted");
    expect(b?.text).toBe(
      "Polling halted: a /usage call reached the model (see log). Clear only after confirming the Claude Code version/flags",
    );
    expect(b?.action).toBe("clear_halt");
  });

  it("shows the stalled banner next", () => {
    const b = bannerFor(
      dash({ stalled_at: 1_700_000_000_000, binary: { path: null, source: null } }),
    );
    expect(b?.kind).toBe("stalled");
    expect(b?.text.startsWith("Poller stalled at ")).toBe(true);
    expect(b?.text.endsWith(" — recovered")).toBe(true);
  });

  it("shows the missing binary banner next", () => {
    const b = bannerFor(dash({ binary: { path: null, source: null } }));
    expect(b?.kind).toBe("no_binary");
    expect(b?.text).toBe("Claude binary not found — set it in Settings");
    expect(b?.action).toBe("open_settings");
  });

  it("shows the no-enabled-accounts banner when every account is disabled", () => {
    const b = bannerFor(
      dash({ accounts: [row({ account: account({ enabled: false }) })] }),
    );
    expect(b?.kind).toBe("no_accounts");
    expect(b?.text).toBe("Polling paused: no enabled accounts");
  });

  it("shows the active banner with the configured gap", () => {
    const b = bannerFor(dash({ gate: "active", interval_secs: 60 }));
    expect(b?.kind).toBe("active");
    expect(b?.text).toBe("Claude running — polling 60 s after each cycle");
  });

  it("shows the idle banner otherwise", () => {
    const b = bannerFor(dash({ gate: "idle" }));
    expect(b?.kind).toBe("idle");
    expect(b?.text).toBe("Idle — will resume when Claude Code starts");
  });

  it("treats an empty account list as no enabled accounts", () => {
    const b = bannerFor(dash({ accounts: [] }));
    expect(b?.kind).toBe("no_accounts");
  });

  it("shows the held banner, with both figures, when automatic refreshes are held", () => {
    const b = bannerFor(dash({ memory_hold: HOLD }));
    expect(b?.kind).toBe("held");
    expect(b?.tone).toBe("info");
    expect(b?.text).toContain(formatBytes(512 * MIB));
    expect(b?.text).toContain(formatBytes(1536 * MIB));
  });

  it("holds outrank active and idle but yield to no_accounts and the faults above it", () => {
    expect(bannerFor(dash({ memory_hold: HOLD, gate: "active" }))?.kind).toBe("held");
    expect(bannerFor(dash({ memory_hold: HOLD, gate: "idle" }))?.kind).toBe("held");
    const noAccounts = dash({ memory_hold: HOLD, accounts: [row({ account: account({ enabled: false }) })] });
    expect(bannerFor(noAccounts)?.kind).toBe("no_accounts");
    expect(bannerFor(dash({ memory_hold: HOLD, binary: { path: null, source: null } }))?.kind).toBe("no_binary");
    expect(bannerFor(dash({ memory_hold: HOLD, stalled_at: 1 }))?.kind).toBe("stalled");
    expect(bannerFor(dash({ memory_hold: HOLD, halted: "guard" }))?.kind).toBe("halted");
  });
});
