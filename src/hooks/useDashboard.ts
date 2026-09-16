import { useCallback, useEffect, useRef, useState } from "react";
import { backend } from "../lib/backend";
import { errorMessage } from "../lib/errors";
import { HISTORY_DAYS } from "../lib/series";
import type { Dashboard, HistoryPoint } from "../lib/types";

const DEBOUNCE_MS = 250;
const TICK_MS = 1000;

interface UseDashboard {
  dashboard: Dashboard | null;
  history: Record<string, HistoryPoint[]>;
  now: number;
  error: string | null;
  refetch: () => void;
}

/**
 * Events are refetch triggers only: the payloads are ignored and the whole
 * dashboard is re-read through commands, so a missed event can never leave
 * the UI wrong. `now` ticks once a second purely for relative-time text.
 */
export function useDashboard(): UseDashboard {
  const [dashboard, setDashboard] = useState<Dashboard | null>(null);
  const [history, setHistory] = useState<Record<string, HistoryPoint[]>>({});
  const [now, setNow] = useState<number>(() => Date.now());
  const [error, setError] = useState<string | null>(null);
  const pending = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Guards against two in-flight get_dashboard calls resolving out of
  // order (a debounced refetch racing a cycle:finished load, say): only
  // the call that is still the most recently *started* one when it
  // resolves is allowed to write dashboard/error.
  const seqRef = useRef(0);

  const load = useCallback(async (): Promise<Dashboard | null> => {
    const seq = ++seqRef.current;
    try {
      const next = await backend().invoke<Dashboard>("get_dashboard");
      if (seq === seqRef.current) {
        setDashboard(next);
        setError(null);
      }
      return next;
    } catch (e) {
      if (seq === seqRef.current) {
        setError(errorMessage(e));
      }
      return null;
    }
  }, []);

  const loadHistoryFor = useCallback(async (accountIds: string[]): Promise<void> => {
    try {
      const entries = await Promise.all(
        accountIds.map(async (id) => {
          const points = await backend().invoke<HistoryPoint[]>("get_history", {
            accountId: id,
            days: HISTORY_DAYS,
          });
          return [id, points] as const;
        }),
      );
      setHistory(Object.fromEntries(entries));
    } catch (e) {
      setError(errorMessage(e));
    }
  }, []);

  const loadWithHistory = useCallback(async (): Promise<void> => {
    const d = await load();
    if (d !== null) await loadHistoryFor(d.accounts.map((r) => r.account.id));
  }, [load, loadHistoryFor]);

  const refetch = useCallback((): void => {
    if (pending.current !== null) {
      clearTimeout(pending.current);
    }
    pending.current = setTimeout(() => {
      pending.current = null;
      void load();
    }, DEBOUNCE_MS);
  }, [load]);

  useEffect(() => {
    void loadWithHistory();
  }, [loadWithHistory]);

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), TICK_MS);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;

    const subscribe = async (name: string, onEvent: () => void): Promise<void> => {
      try {
        const off = await backend().listen(name, onEvent);
        if (cancelled) {
          off();
        } else {
          unlisteners.push(off);
        }
      } catch (e) {
        setError(errorMessage(e));
        console.warn("dashboard: could not subscribe", name, e);
      }
    };

    const attach = async (): Promise<void> => {
      const names = ["usage:updated", "gate:changed", "poller:stalled"];
      for (const name of names) {
        await subscribe(name, () => refetch());
      }
      await subscribe("cycle:finished", () => void loadWithHistory());
    };

    void attach();
    return () => {
      cancelled = true;
      for (const off of unlisteners) {
        off();
      }
      if (pending.current !== null) {
        clearTimeout(pending.current);
        pending.current = null;
      }
    };
  }, [refetch, loadWithHistory]);

  return { dashboard, history, now, error, refetch };
}
