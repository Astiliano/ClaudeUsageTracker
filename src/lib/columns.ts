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

export function isColumnOrder(v: unknown): v is ColumnKey[] {
  if (!Array.isArray(v) || v.length !== DEFAULT_ORDER.length) return false;
  const seen = new Set<string>();
  for (const item of v) {
    if (typeof item !== "string" || !(item in COLUMNS) || seen.has(item)) return false;
    seen.add(item);
  }
  return true;
}
