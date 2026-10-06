import { describe, expect, it } from "vitest";
import { HISTORY_EVENTS, REFETCH_EVENTS, SYSTEM_EVENTS } from "./events";

describe("event names", () => {
  it("lists exactly the spec's names", () => {
    expect([...REFETCH_EVENTS]).toEqual(["usage:updated", "gate:changed", "poller:stalled", "memory:hold", "settings:applied"]);
    expect([...HISTORY_EVENTS]).toEqual(["cycle:finished"]);
    expect([...SYSTEM_EVENTS]).toEqual(["system:sampled"]);
  });
});
