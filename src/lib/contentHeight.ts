/** The slice of an element the measurement reads (`offsetHeight` is the no-observer fallback). */
export type MeasuredElement = { offsetHeight: number };

/**
 * The slice of `ResizeObserver` the measurement drives, so a node test can fake it.
 * Generic in the element type so the real `ResizeObserver` (which observes an
 * `Element`) fits with `E = HTMLElement` and a fake fits with the default.
 */
export type ContentObserverCtor<E extends MeasuredElement = MeasuredElement> = new (
  cb: (entries: readonly { borderBoxSize: readonly { blockSize: number }[] }[]) => void,
) => {
  observe(el: E, opts: { box: "border-box" }): void;
  disconnect(): void;
};

/** One layout unit: Chromium sums lengths in 1/64 px. */
const LAYOUT_UNIT = 1 / 64;

/** The reported content height: the ceiling of the border box less one layout unit. */
function reportable(h: number): number {
  return Math.ceil(h - LAYOUT_UNIT);
}

/**
 * Reports the element's border-box height, in the element's own unzoomed px, every
 * time it changes. With an observer the first report is the observer's initial
 * callback; without one it warns once and reports `offsetHeight` once. Returns the
 * unsubscribe function.
 */
export function subscribeContentHeight<E extends MeasuredElement>(
  element: E,
  observerCtor: ContentObserverCtor<E> | undefined,
  report: (h: number) => void,
): () => void {
  let last: number | null = null;
  let active = true;
  const deliver = (h: number): void => {
    if (!active) return;
    const next = reportable(h);
    if (!Number.isFinite(next) || next <= 0 || next === last) return;
    last = next;
    report(next);
  };

  if (observerCtor === undefined) {
    console.warn("content height: ResizeObserver unavailable; reporting once");
    deliver(element.offsetHeight);
    return () => {
      active = false;
    };
  }

  const observer = new observerCtor((entries) => {
    const entry = entries[entries.length - 1];
    const size = entry?.borderBoxSize[0];
    if (size === undefined) return;
    deliver(size.blockSize);
  });
  observer.observe(element, { box: "border-box" });
  return () => {
    active = false;
    observer.disconnect();
  };
}

/**
 * The gate the hook calls: no observer and no report while disabled (the dashboard
 * is still loading) or while there is no element to measure.
 */
export function attachContentHeight<E extends MeasuredElement>(
  enabled: boolean,
  element: E | null,
  observerCtor: ContentObserverCtor<E> | undefined,
  report: (h: number) => void,
): () => void {
  if (!enabled || element === null) return () => undefined;
  return subscribeContentHeight(element, observerCtor, report);
}
