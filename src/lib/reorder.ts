/**
 * Moves the item at `from` to `to`, returning a new array. Purely
 * presentational: the caller is responsible for persisting the result
 * (`invoke("reorder_accounts", { ids })`). Out-of-range indices are a no-op
 * that still returns a fresh copy, so callers can always treat the result as
 * the new list to render.
 */
export function moveItem<T>(list: readonly T[], from: number, to: number): T[] {
  const copy = [...list];
  if (
    from < 0 ||
    from >= copy.length ||
    to < 0 ||
    to >= copy.length
  ) {
    return copy;
  }
  const [item] = copy.splice(from, 1);
  copy.splice(to, 0, item);
  return copy;
}
