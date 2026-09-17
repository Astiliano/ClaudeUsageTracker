import { useCallback, useEffect, useRef, useState } from "react";
import { backend } from "../lib/backend";
import { errorMessage } from "../lib/errors";
import type { SystemReport } from "../lib/types";

interface UseSystem {
  report: SystemReport | null;
  error: string | null;
}

/**
 * The sampler's figures. `system:sampled` is a refetch trigger only: the
 * payload is empty and the whole report is re-read, so a missed event can
 * never leave the line wrong.
 *
 * A successful response always replaces the report, including one whose
 * `stats` is null — that is how the sampler says it has stopped. Only a
 * rejected invoke keeps the previous report, and it sets `error` rather than
 * raising a toast: a failure every 5 s would be a toast storm.
 */
export function useSystem(): UseSystem {
  const [report, setReport] = useState<SystemReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Guards against two in-flight get_system calls resolving out of order:
  // only the call that is still the most recently started one when it
  // resolves may write report/error.
  const seqRef = useRef(0);

  const load = useCallback(async (): Promise<void> => {
    const seq = ++seqRef.current;
    try {
      const next = await backend().invoke<SystemReport>("get_system");
      if (seq === seqRef.current) {
        setReport(next);
        setError(null);
      }
    } catch (e) {
      if (seq === seqRef.current) {
        setError(errorMessage(e));
      }
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | null = null;

    const attach = async (): Promise<void> => {
      try {
        const unlisten = await backend().listen("system:sampled", () => void load());
        if (cancelled) {
          unlisten();
        } else {
          off = unlisten;
        }
      } catch (e) {
        setError(errorMessage(e));
        console.warn("system: could not subscribe", e);
      }
    };

    void attach();
    return () => {
      cancelled = true;
      if (off !== null) off();
    };
  }, [load]);

  return { report, error };
}
