import { describe, expect, it } from "vitest";
import { statusPill } from "./pill";
import type { Account, AccountRow, Outcome, SnapshotDto } from "./types";

const NOW = 1_700_000_000_000;

function account(over: Partial<Account> = {}): Account {
  return {
    id: "a1",
    label: "claude3",
    config_dir: "/home/josh/.claude3",
    enabled: true,
    disabled_reason: null,
    is_default: false,
    created_at: 0,
    sort_order: 0,
    ...over,
  };
}

function snapshot(outcome: Outcome): SnapshotDto {
  return {
    id: 1,
    account_id: "a1",
    taken_at: NOW - 1000,
    outcome,
    session: null,
    week_all: null,
    week_models: [],
    error: outcome === "ok" ? null : "boom",
    duration_ms: 10,
  };
}

function row(over: Partial<AccountRow> = {}): AccountRow {
  return {
    account: account(),
    latest: snapshot("ok"),
    backoff_until: null,
    ...over,
  };
}

describe("statusPill", () => {
  it("shows disabled ahead of everything else", () => {
    const pill = statusPill(
      row({
        account: account({ enabled: false, disabled_reason: "user" }),
        backoff_until: NOW + 240_000,
        latest: snapshot("spawn_error"),
      }),
      NOW,
    );
    expect(pill.kind).toBe("disabled");
    expect(pill.label).toBe("disabled");
    expect(pill.tooltip).toBe("disabled by you");
  });

  it("explains a guard-tripped disable in the tooltip", () => {
    const pill = statusPill(
      row({
        account: account({ enabled: false, disabled_reason: "guard_tripped" }),
      }),
      NOW,
    );
    expect(pill.tooltip).toBe("disabled because the envelope guard tripped");
  });

  it("shows backing off ahead of the latest outcome", () => {
    const pill = statusPill(
      row({ backoff_until: NOW + 4 * 60_000, latest: snapshot("timeout") }),
      NOW,
    );
    expect(pill.kind).toBe("backoff");
    expect(pill.label).toBe("backing off (next in 4 min)");
  });

  it("rounds a sub-minute cooldown up to one minute", () => {
    const pill = statusPill(row({ backoff_until: NOW + 5_000 }), NOW);
    expect(pill.label).toBe("backing off (next in 1 min)");
  });

  it("ignores a cooldown that has already expired", () => {
    const pill = statusPill(
      row({ backoff_until: NOW - 1, latest: snapshot("ok") }),
      NOW,
    );
    expect(pill.kind).toBe("outcome");
    expect(pill.label).toBe("ok");
  });

  it("renders each outcome with its own wording", () => {
    const cases: Array<[Outcome, string]> = [
      ["ok", "ok"],
      ["no_usage_data", "no data — log in?"],
      ["parse_error", "parse error"],
      ["spawn_error", "spawn error"],
      ["timeout", "timeout"],
      ["guard_tripped", "guard tripped"],
    ];
    for (const [outcome, label] of cases) {
      const pill = statusPill(row({ latest: snapshot(outcome) }), NOW);
      expect(pill.kind).toBe("outcome");
      expect(pill.label).toBe(label);
    }
  });

  it("tones ok success, a guard trip error, other failures warn", () => {
    expect(statusPill(row({ latest: snapshot("ok") }), NOW).tone).toBe("success");
    expect(statusPill(row({ latest: snapshot("guard_tripped") }), NOW).tone).toBe(
      "error",
    );
    for (const outcome of [
      "no_usage_data",
      "parse_error",
      "spawn_error",
      "timeout",
    ] as const) {
      expect(statusPill(row({ latest: snapshot(outcome) }), NOW).tone).toBe("warn");
    }
  });

  it("tones backoff warn and the two resting states neutral", () => {
    expect(statusPill(row({ backoff_until: NOW + 200_000 }), NOW).tone).toBe("warn");
    expect(statusPill(row({ latest: null }), NOW).tone).toBe("neutral");
    expect(
      statusPill(row({ account: account({ enabled: false }) }), NOW).tone,
    ).toBe("neutral");
  });

  it("shows no data yet when the account has never been polled", () => {
    const pill = statusPill(row({ latest: null }), NOW);
    expect(pill.kind).toBe("pending");
    expect(pill.label).toBe("not polled yet");
  });

  it("carries the stored error as the tooltip for a failure", () => {
    const pill = statusPill(row({ latest: snapshot("parse_error") }), NOW);
    expect(pill.tooltip).toBe("boom");
  });
});
