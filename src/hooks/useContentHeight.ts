import type { RefObject } from "react";
import { useEffect, useRef, useState } from "react";
import { backend } from "../lib/backend";
import { attachContentHeight } from "../lib/contentHeight";
import {
  createFitController,
  initialFitState,
  zoomContentH,
  type FitController,
  type FitState,
} from "../lib/fit";

/**
 * Reports the element's content height to the backend and returns the content height
 * the zoom divides the window height by (null until the first fit outcome). Wiring
 * only: the measurement is `attachContentHeight`, the decisions are `createFitController`.
 */
export function useContentHeight(
  ref: RefObject<HTMLElement | null>,
  enabled: boolean,
  viewport: { width: number; height: number },
): number | null {
  const [state, setState] = useState<FitState>(initialFitState);
  const controllerRef = useRef<FitController | null>(null);

  // Built inside the effect, not in useRef(...): StrictMode runs mount, cleanup and
  // mount again, and a controller disposed by the first cleanup must not be reused.
  useEffect(() => {
    controllerRef.current = createFitController({
      send: (c) => backend().setContentHeight(c),
      publish: setState,
      warn: (message, error) => console.warn(message, error),
    });
    return () => {
      controllerRef.current?.dispose();
      controllerRef.current = null;
    };
  }, []);

  useEffect(
    () =>
      attachContentHeight<HTMLElement>(enabled, ref.current, globalThis.ResizeObserver, (c) =>
        controllerRef.current?.onMeasured(c),
      ),
    // `ref` is a stable ref object; its element is read when `enabled` flips.
    [enabled],
  );

  useEffect(() => {
    controllerRef.current?.onViewport(viewport);
  }, [viewport]);

  return zoomContentH(state);
}
