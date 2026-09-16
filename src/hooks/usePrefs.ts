import { useCallback, useState } from "react";
import { type Prefs, type PrefsStore, loadPrefs, savePrefs } from "../lib/prefs";

function storage(): PrefsStore | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch (e) {
    console.warn("prefs: localStorage unavailable", e);
    return null;
  }
}

export interface UsePrefs {
  prefs: Prefs;
  update: (patch: Partial<Prefs>) => void;
}

export function usePrefs(): UsePrefs {
  const [prefs, setPrefs] = useState<Prefs>(() => loadPrefs(storage()));
  const update = useCallback((patch: Partial<Prefs>): void => {
    setPrefs((prev) => {
      const next = { ...prev, ...patch };
      savePrefs(storage(), next);
      return next;
    });
  }, []);
  return { prefs, update };
}
