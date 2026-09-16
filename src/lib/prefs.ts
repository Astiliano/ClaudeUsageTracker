import { type ColumnKey, DEFAULT_ORDER, normalizeColumnOrder } from "./columns";
import { type FontKey, type SizeKey, isFontKey, isSizeKey } from "./theme";

export interface Prefs {
  font: FontKey;
  size: SizeKey;
  columnOrder: ColumnKey[];
}

export const PREFS_KEY = "usage-tracker.prefs.v1";

export const DEFAULT_PREFS: Readonly<Prefs> = {
  font: "system",
  size: "md",
  columnOrder: [...DEFAULT_ORDER],
};

/** The subset of the Web Storage API the app touches; injectable for tests. */
export interface PrefsStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function fresh(): Prefs {
  return { font: DEFAULT_PREFS.font, size: DEFAULT_PREFS.size, columnOrder: [...DEFAULT_ORDER] };
}

/** Per-field validation: one bad field never discards the others. */
export function parsePrefs(raw: string | null): Prefs {
  const out = fresh();
  if (raw === null) return out;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return out;
  }
  if (typeof parsed !== "object" || parsed === null) return out;
  const rec = parsed as Record<string, unknown>;
  if (isFontKey(rec.font)) out.font = rec.font;
  if (isSizeKey(rec.size)) out.size = rec.size;
  const order = normalizeColumnOrder(rec.columnOrder);
  if (order !== null) out.columnOrder = order;
  if (Array.isArray(rec.columnOrder) && JSON.stringify(order) !== JSON.stringify(rec.columnOrder)) {
    console.warn("prefs: stored column order was invalid or out of date; normalizing", rec.columnOrder);
  }
  return out;
}

export function loadPrefs(store: PrefsStore | null): Prefs {
  if (store === null) return fresh();
  try {
    return parsePrefs(store.getItem(PREFS_KEY));
  } catch (e) {
    console.warn("prefs: could not read storage, using defaults", e);
    return fresh();
  }
}

export function savePrefs(store: PrefsStore | null, prefs: Prefs): void {
  if (store === null) return;
  try {
    store.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch (e) {
    console.warn("prefs: could not write storage", e);
  }
}
