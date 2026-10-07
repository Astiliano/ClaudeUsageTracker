import { useEffect, useState } from "react";

export interface Viewport {
  width: number;
  height: number;
}

/** The part of `window` the subscription reads and listens to. */
export interface ViewportSource {
  innerWidth: number;
  innerHeight: number;
  addEventListener(type: "resize", listener: () => void): void;
  removeEventListener(type: "resize", listener: () => void): void;
}

export function readViewport(source: ViewportSource): Viewport {
  return { width: source.innerWidth, height: source.innerHeight };
}

/**
 * Calls `onChange` with the current size on every window `resize`, and on
 * every resize of the document element when a ResizeObserver is given.
 * Both are needed: the document element's width follows the viewport, but its
 * height is the content height, so a height-only drag of the window edge
 * never resizes it and only the `resize` event reports it. Returns the
 * unsubscribe function.
 */
export function subscribeViewport(
  source: ViewportSource,
  onChange: (next: Viewport) => void,
  observed: { element: Element; observer: typeof ResizeObserver } | null,
): () => void {
  const update = (): void => onChange(readViewport(source));
  source.addEventListener("resize", update);
  const observer = observed === null ? null : new observed.observer(update);
  observer?.observe(observed?.element as Element);
  return () => {
    source.removeEventListener("resize", update);
    observer?.disconnect();
  };
}

function currentViewport(): Viewport {
  return typeof window === "undefined" ? { width: 0, height: 0 } : readViewport(window);
}

/**
 * `window.innerWidth` and `innerHeight`, re-read on window resize and on
 * document-element resize. innerWidth includes the vertical scrollbar, so a
 * list that grows tall enough to scroll cannot flip the layout back and forth
 * at a breakpoint. The state keeps its identity while the size is unchanged.
 */
export function useViewport(): Viewport {
  const [viewport, setViewport] = useState<Viewport>(currentViewport);
  useEffect(() => {
    const observed =
      typeof ResizeObserver === "undefined"
        ? null
        : { element: document.documentElement, observer: ResizeObserver };
    if (observed === null) console.warn("layout: ResizeObserver unavailable; viewport follows window resize only");
    return subscribeViewport(window, (next) => {
      setViewport((prev) => (prev.width === next.width && prev.height === next.height ? prev : next));
    }, observed);
  }, []);
  return viewport;
}
