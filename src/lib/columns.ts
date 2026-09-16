/** Column registry from docs/design/usage-tracker-kit.js (COLUMNS). Cell
 *  rendering lives in AccountRow.tsx, keyed on ColumnKey. */
export type ColumnKey = "account" | "session" | "week" | "model" | "spark" | "updated";
export interface ColumnDef { label: string; width: string }

export const COLUMNS: Record<ColumnKey, ColumnDef> = {
  account: { label: "Account", width: "minmax(150px,1fr)" },
  session: { label: "Session", width: "minmax(86px,1.5fr)" },
  week: { label: "Week (all)", width: "minmax(86px,1.5fr)" },
  model: { label: "Per model", width: "minmax(86px,1.5fr)" },
  spark: { label: "7 days", width: "76px" },
  updated: { label: "Updated", width: "82px" },
};

export const DEFAULT_ORDER: readonly ColumnKey[] = [
  "account", "session", "week", "model", "spark", "updated",
];

/** Grip column first, per-row action column last. */
const LEAD_WIDTH = "26px";
const TRAIL_WIDTH = "62px";

export function gridTemplate(order: readonly ColumnKey[]): string {
  return [LEAD_WIDTH, ...order.map((k) => COLUMNS[k].width), TRAIL_WIDTH].join(" ");
}

function isColumnKey(v: string): v is ColumnKey {
  return v in COLUMNS;
}

/**
 * Migrates a stored column order into a valid one: keeps known keys in
 * their stored order (dropping unknown or duplicate keys), then appends any
 * column missing from the stored value in default order. Returns null only
 * when `v` is not an array of strings at all.
 */
export function normalizeColumnOrder(v: unknown): ColumnKey[] | null {
  if (!Array.isArray(v)) return null;
  const strs: string[] = [];
  for (const item of v) {
    if (typeof item !== "string") return null;
    strs.push(item);
  }
  const seen = new Set<string>();
  const known: ColumnKey[] = [];
  for (const item of strs) {
    if (isColumnKey(item) && !seen.has(item)) {
      seen.add(item);
      known.push(item);
    }
  }
  for (const key of DEFAULT_ORDER) {
    if (!seen.has(key)) known.push(key);
  }
  return known;
}
