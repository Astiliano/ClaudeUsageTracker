import { useLayoutEffect, useRef } from "react";
import type { RefObject } from "react";
import { chartHeightPx } from "../lib/chart";

/**
 * Sizes the history chart so row + drawer fill the viewport. Writes
 * `chart.style.height` directly (no React state) in a layout effect, so no
 * frame paints the unsized chart. After the first write only, scrolls the
 * row to the viewport top so the drawer ends at the fold; later recomputes
 * (drawer resize, window resize, zoom change) never scroll.
 */
export function useChartHeight(
  drawerRef: RefObject<HTMLDivElement | null>,
  chartRef: RefObject<HTMLDivElement | null>,
  rowRef: RefObject<HTMLDivElement | null>,
  zoom: number,
): void {
  const scrolled = useRef(false);
  useLayoutEffect(() => {
    const drawer = drawerRef.current;
    const chart = chartRef.current;
    if (drawer === null || chart === null) return undefined;

    const apply = (): void => {
      const chromePx = drawer.getBoundingClientRect().height - chart.getBoundingClientRect().height;
      chart.style.height = `${chartHeightPx({ viewportPx: window.innerHeight, chromePx, zoom })}px`;
    };

    apply();
    if (!scrolled.current) {
      scrolled.current = true;
      rowRef.current?.scrollIntoView({ block: "start" });
    }

    window.addEventListener("resize", apply);
    let observer: ResizeObserver | null = null;
    if (typeof ResizeObserver === "undefined") {
      console.warn("chart: ResizeObserver unavailable; chart height follows window resize only");
    } else {
      observer = new ResizeObserver(apply);
      observer.observe(drawer);
    }
    return () => {
      window.removeEventListener("resize", apply);
      observer?.disconnect();
    };
  }, [drawerRef, chartRef, rowRef, zoom]);
}
