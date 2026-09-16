import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
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

  const load = useCallback(async (): Promise<void> => {
    try {
      const next = await invoke<Dashboard>("get_dashboard");
      setDashboard(next);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const loadHistory = useCallback(async (): Promise<void> => {
    try {
      const current = await invoke<Dashboard>("get_dashboard");
      const entries = await Promise.all(
        current.accounts.map(async (row) => {
          const points = await invoke<HistoryPoint[]>("get_history", {
            accountId: row.account.id,
          });
          return [row.account.id, points] as const;
        }),
      );
      setHistory(Object.fromEntries(entries));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

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
    void load();
    void loadHistory();
  }, [load, loadHistory]);

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), TICK_MS);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;

    const attach = async (): Promise<void> => {
      const names = ["usage:updated", "gate:changed", "poller:stalled"];
      for (const name of names) {
        const off = await listen(name, () => refetch());
        if (cancelled) {
          off();
        } else {
          unlisteners.push(off);
        }
      }
      const offCycle = await listen("cycle:finished", () => {
        refetch();
        void loadHistory();
      });
      if (cancelled) {
        offCycle();
      } else {
        unlisteners.push(offCycle);
      }
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
  }, [refetch, loadHistory]);

  return { dashboard, history, now, error, refetch };
}
