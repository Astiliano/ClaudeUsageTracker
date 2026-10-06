export const REFETCH_EVENTS = [
  "usage:updated",
  "gate:changed",
  "poller:stalled",
  "memory:hold",
  "settings:applied",
] as const;

export const HISTORY_EVENTS = ["cycle:finished"] as const;

export const SYSTEM_EVENTS = ["system:sampled"] as const;
