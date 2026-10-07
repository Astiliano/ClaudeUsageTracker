import { BASE_WIDTH } from "./layout";

export const FIT_OUTCOMES = ["applied", "alreadyFitted", "capped", "skippedMaximized", "skippedMinimized", "deferred"] as const;
export type FitOutcome = (typeof FIT_OUTCOMES)[number];

/**
 * The owner of the zoom's content height. `cEff` is the content height the window
 * is known to fit, `transit` a fitted height whose resize has not been seen yet,
 * `lastMeasured` the newest report, `retry` whether a resend is wanted, `viewport`
 * the last viewport event in CSS px.
 */
export type FitState = {
  cEff: number | null;
  transit: number | null;
  lastMeasured: number | null;
  retry: boolean;
  viewport: { w: number; h: number } | null;
};

export const initialFitState: FitState = {
  cEff: null,
  transit: null,
  lastMeasured: null,
  retry: false,
  viewport: null,
};

export type FitEvent =
  | { kind: "measured"; c: number }
  | { kind: "outcome"; c: number; outcome: FitOutcome }
  | { kind: "failed"; c: number }
  | { kind: "viewport"; w: number; h: number };

/**
 * The window as last seen is tall enough for content `c` at the width term, within
 * one width px. Integer arithmetic: viewport and `c` are integers and the products
 * stay far below 2^53, so the boundary cannot flip on float rounding.
 */
function holds(state: FitState, c: number): boolean {
  const v = state.viewport;
  return v !== null && v.h * BASE_WIDTH >= (v.w - 1) * c;
}

function onOutcome(state: FitState, c: number, outcome: FitOutcome): FitState {
  switch (outcome) {
    case "applied":
      return holds(state, c)
        ? { ...state, retry: false, cEff: c, transit: null }
        : { ...state, retry: false, transit: c };
    case "capped":
      return { ...state, retry: false, cEff: c, transit: null };
    case "alreadyFitted":
      return { ...state, retry: false, cEff: c, transit: null };
    case "skippedMaximized":
    case "skippedMinimized":
      return { ...state, retry: true, cEff: c, transit: null };
    case "deferred":
      return { ...state, retry: true };
  }
}

export function fitReducer(state: FitState, event: FitEvent): FitState {
  switch (event.kind) {
    case "measured":
      return { ...state, lastMeasured: event.c };
    case "outcome":
      return event.c === state.lastMeasured ? onOutcome(state, event.c, event.outcome) : state;
    case "failed":
      return event.c === state.lastMeasured
        ? { ...state, retry: false, cEff: event.c, transit: null }
        : state;
    case "viewport": {
      const next = { ...state, viewport: { w: event.w, h: event.h } };
      return next.transit !== null && holds(next, next.transit)
        ? { ...next, cEff: next.transit, transit: null }
        : next;
    }
  }
}

/**
 * The content height the zoom divides the window height by: null (width term only)
 * until the first outcome other than `deferred`, then the smallest of the non-null
 * `cEff`, `transit` and `lastMeasured`, which keeps the zoom at the width term
 * while a fit is in flight, in either arrival order of the resize and the reply.
 */
export function zoomContentH(state: FitState): number | null {
  if (state.cEff === null && state.transit === null) return null;
  const heights = [state.cEff, state.transit, state.lastMeasured].filter(
    (c): c is number => c !== null,
  );
  return Math.min(...heights);
}

export function shouldResend(state: FitState): boolean {
  return state.retry && state.lastMeasured !== null;
}

/** The quiet time after the last viewport event before a deferred or skipped fit is resent. */
export const RESEND_DEBOUNCE_MS = 200;

export type FitController = {
  onMeasured(c: number): void;
  onViewport(v: { width: number; height: number }): void;
  dispose(): void;
};

/**
 * The glue between the measurement, the command and the zoom, with injected effects.
 * It is the one owner of `FitState`: it applies `fitReducer` and hands each new state
 * to `publish`. After `dispose` nothing is sent, applied or published.
 */
export function createFitController(deps: {
  send: (c: number) => Promise<FitOutcome>;
  publish: (state: FitState) => void;
  warn: (message: string, error: unknown) => void;
}): FitController {
  let state = initialFitState;
  let disposed = false;
  let inFlight = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const apply = (event: FitEvent): void => {
    state = fitReducer(state, event);
    deps.publish(state);
  };

  const call = (c: number): void => {
    inFlight += 1;
    let reply: Promise<FitOutcome>;
    try {
      reply = deps.send(c);
    } catch (e) {
      reply = Promise.reject(e);
    }
    reply
      .then(
        (outcome) => {
          inFlight -= 1;
          if (!disposed) apply({ kind: "outcome", c, outcome });
        },
        (e: unknown) => {
          inFlight -= 1;
          if (disposed) return;
          deps.warn("content height: fit failed", e);
          apply({ kind: "failed", c });
        },
      )
      .catch((e: unknown) => {
        deps.warn("content height: applying the fit reply failed", e);
      });
  };

  const onTimer = (): void => {
    timer = null;
    if (disposed || !shouldResend(state)) return;
    if (inFlight > 0) {
      arm();
      return;
    }
    if (state.lastMeasured !== null) call(state.lastMeasured);
  };

  const arm = (): void => {
    if (timer !== null) clearTimeout(timer);
    timer = setTimeout(onTimer, RESEND_DEBOUNCE_MS);
  };

  return {
    onMeasured(c) {
      if (disposed) return;
      apply({ kind: "measured", c });
      call(c);
    },
    onViewport(v) {
      if (disposed) return;
      apply({ kind: "viewport", w: v.width, h: v.height });
      if (shouldResend(state)) arm();
    },
    dispose() {
      disposed = true;
      if (timer !== null) clearTimeout(timer);
      timer = null;
    },
  };
}
