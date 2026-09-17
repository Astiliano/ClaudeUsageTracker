import { useEffect, useState } from "react";

function currentWidth(): number {
  return typeof window === "undefined" ? 0 : window.innerWidth;
}

/**
 * `window.innerWidth`, re-read whenever the document element resizes.
 * innerWidth includes the vertical scrollbar, so a list that grows tall
 * enough to scroll cannot flip the layout back and forth at a breakpoint.
 */
export function useViewport(): number {
  const [width, setWidth] = useState<number>(currentWidth);
  useEffect(() => {
    if (typeof ResizeObserver === "undefined") {
      console.warn("layout: ResizeObserver unavailable; viewport width will not update");
      return undefined;
    }
    const observer = new ResizeObserver(() => setWidth(currentWidth()));
    observer.observe(document.documentElement);
    return () => observer.disconnect();
  }, []);
  return width;
}
