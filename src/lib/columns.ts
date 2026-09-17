import { moveItem } from "./reorder";

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

/** Columns that can never be hidden. */
export const ALWAYS_VISIBLE: readonly ColumnKey[] = ["account"];

/** `.row-grid` / `.thead` gap in styles.css; gridMinWidth depends on it. */
export const GRID_GAP = 10;

export function visibleColumns(order: readonly ColumnKey[], hidden: readonly ColumnKey[]): ColumnKey[] {
  return order.filter((k) => ALWAYS_VISIBLE.includes(k) || !hidden.includes(k));
}

/**
 * `from`/`to` index the VISIBLE list. Only the dragged key moves, to the
 * full-order position of the key currently at visible index `to`; hidden
 * keys stay exactly where they are, so a drag made while columns are hidden
 * never rearranges columns the user could not see. With nothing hidden this
 * is `moveItem`. Out-of-range indices return a copy of `order`.
 */
export function moveVisible(
  order: readonly ColumnKey[],
  hidden: readonly ColumnKey[],
  from: number,
  to: number,
): ColumnKey[] {
  const visible = visibleColumns(order, hidden);
  const fromKey = visible[from];
  const toKey = visible[to];
  if (fromKey === undefined || toKey === undefined) return [...order];
  return moveItem(order, order.indexOf(fromKey), order.indexOf(toKey));
}

/**
 * Known keys minus `account`, de-duplicated, in the stored order. Null when
 * `v` is not an array of strings at all (the caller warns and defaults).
 */
export function normalizeHiddenColumns(v: unknown): ColumnKey[] | null {
  if (!Array.isArray(v)) return null;
  const out: ColumnKey[] = [];
  for (const item of v) {
    if (typeof item !== "string") return null;
    if (isColumnKey(item) && !ALWAYS_VISIBLE.includes(item) && !out.includes(item)) out.push(item);
  }
  return out;
}

/** Minimum px the grid needs: every track's px floor plus the gaps between tracks. */
export function gridMinWidth(order: readonly ColumnKey[]): number {
  const px = (width: string): number => {
    const m = /(\d+(?:\.\d+)?)px/.exec(width);
    return m === null ? 0 : Number(m[1]);
  };
  const tracks = [LEAD_WIDTH, ...order.map((k) => COLUMNS[k].width), TRAIL_WIDTH];
  return tracks.reduce((sum, w) => sum + px(w), 0) + GRID_GAP * (tracks.length - 1);
}
