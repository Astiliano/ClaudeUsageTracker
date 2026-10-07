import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  FIT_OUTCOMES,
  RESEND_DEBOUNCE_MS,
  createFitController,
  fitReducer,
  initialFitState,
  shouldResend,
  zoomContentH,
  type FitEvent,
  type FitOutcome,
  type FitState,
} from "./fit";
import { windowZoom } from "./layout";

const state = (over: Partial<FitState> = {}): FitState => ({ ...initialFitState, ...over });

const run = (start: FitState, ...events: FitEvent[]): FitState =>
  events.reduce((s, e) => fitReducer(s, e), start);

const measured = (c: number): FitEvent => ({ kind: "measured", c });
const outcome = (c: number, o: FitOutcome): FitEvent => ({ kind: "outcome", c, outcome: o });
const viewport = (w: number, h: number): FitEvent => ({ kind: "viewport", w, h });

describe("FIT_OUTCOMES", () => {
  it("FIT_OUTCOMES lists the six outcomes", () => {
    expect([...FIT_OUTCOMES]).toEqual([
      "applied",
      "alreadyFitted",
      "capped",
      "skippedMaximized",
      "skippedMinimized",
      "deferred",
    ]);
  });
});

describe("fitReducer", () => {
  it("the initial state is all null with no retry", () => {
    expect(initialFitState).toEqual({
      cEff: null,
      transit: null,
      lastMeasured: null,
      retry: false,
      viewport: null,
    });
  });

  it("measured sets lastMeasured", () => {
    expect(run(initialFitState, measured(640)).lastMeasured).toBe(640);
  });

  it("a stale outcome is ignored", () => {
    const s = state({ lastMeasured: 700, retry: true });
    expect(run(s, outcome(640, "applied"))).toEqual(s);
    expect(run(s, outcome(640, "capped"))).toEqual(s);
  });

  it("a stale failure is ignored", () => {
    const s = state({ lastMeasured: 700, retry: true });
    expect(run(s, { kind: "failed", c: 640 })).toEqual(s);
  });

  it("failed sets cEff", () => {
    const s = state({ lastMeasured: 640, transit: 600, retry: true });
    expect(run(s, { kind: "failed", c: 640 })).toEqual({
      ...s,
      cEff: 640,
      transit: null,
      retry: false,
    });
  });

  it("applied settles at once when the stored viewport holds it", () => {
    const s = run(
      state({ cEff: 900, lastMeasured: 900, viewport: { w: 1225, h: 1125 } }),
      measured(640),
      outcome(640, "applied"),
    );
    expect(s.cEff).toBe(640);
    expect(s.transit).toBeNull();
    expect(s.retry).toBe(false);
  });

  it("applied waits in transit until a viewport that holds it", () => {
    const start = state({ cEff: 640, lastMeasured: 640, viewport: { w: 1225, h: 800 } });
    const waiting = run(start, measured(900), outcome(900, "applied"));
    expect(waiting.cEff).toBe(640);
    expect(waiting.transit).toBe(900);
    const same = fitReducer(waiting, viewport(1225, 800));
    expect(same.cEff).toBe(640);
    expect(same.transit).toBe(900);
    const settled = fitReducer(same, viewport(1225, 1125));
    expect(settled.cEff).toBe(900);
    expect(settled.transit).toBeNull();
  });

  it("applied clears retry", () => {
    const s = run(state({ lastMeasured: 640, retry: true }), outcome(640, "applied"));
    expect(s.retry).toBe(false);
  });

  it("holds tolerates one width px", () => {
    const s = run(state({ viewport: { w: 1226, h: 1125 } }), measured(900), outcome(900, "applied"));
    expect(s.cEff).toBe(900);
    expect(s.transit).toBeNull();
  });

  it("holds rejects one height px short", () => {
    const s = run(state({ viewport: { w: 1226, h: 1124 } }), measured(900), outcome(900, "applied"));
    expect(s.cEff).toBeNull();
    expect(s.transit).toBe(900);
  });

  it("applied with no viewport yet waits in transit", () => {
    const s = run(initialFitState, measured(640), outcome(640, "applied"));
    expect(s.cEff).toBeNull();
    expect(s.transit).toBe(640);
  });

  it("capped settles at once", () => {
    const s = run(
      state({ viewport: { w: 1225, h: 800 }, retry: true }),
      measured(900),
      outcome(900, "capped"),
    );
    expect(s.cEff).toBe(900);
    expect(s.transit).toBeNull();
    expect(s.retry).toBe(false);
  });

  it("alreadyFitted settles and clears retry", () => {
    const s = run(
      state({ transit: 700, retry: true }),
      measured(640),
      outcome(640, "alreadyFitted"),
    );
    expect(s.cEff).toBe(640);
    expect(s.transit).toBeNull();
    expect(s.retry).toBe(false);
  });

  it.each(["skippedMaximized", "skippedMinimized"] as const)("%s settles and asks for a retry", (o) => {
    const s = run(state({ transit: 700 }), measured(640), outcome(640, o));
    expect(s.cEff).toBe(640);
    expect(s.transit).toBeNull();
    expect(s.retry).toBe(true);
  });

  it("deferred sets retry only", () => {
    const s = state({ cEff: 640, transit: 700, lastMeasured: 700 });
    expect(run(s, outcome(700, "deferred"))).toEqual({ ...s, retry: true });
  });

  it("a viewport event with no transit only stores the viewport", () => {
    const s = fitReducer(state({ cEff: 640 }), viewport(1225, 800));
    expect(s).toEqual(state({ cEff: 640, viewport: { w: 1225, h: 800 } }));
  });

  it("two reports in flight: the first fit's resize does not settle the second", () => {
    const w = 1225;
    let s = state({ cEff: 640, lastMeasured: 640, viewport: { w, h: 800 } });
    const zoomAt = (): number =>
      windowZoom(w, s.viewport?.h ?? 0, zoomContentH(s));
    expect(zoomAt()).toBeCloseTo(1.25, 10);
    s = fitReducer(s, measured(653));
    expect(zoomAt()).toBeCloseTo(1.25, 10);
    s = fitReducer(s, measured(666));
    expect(zoomAt()).toBeCloseTo(1.25, 10);
    s = fitReducer(s, outcome(653, "applied"));
    expect(s.transit).toBeNull();
    expect(s.cEff).toBe(640);
    expect(zoomAt()).toBeCloseTo(1.25, 10);
    s = fitReducer(s, viewport(w, 816.25));
    expect(s.cEff).toBe(640);
    expect(zoomAt()).toBeCloseTo(1.25, 10);
    s = fitReducer(s, outcome(666, "applied"));
    expect(s.transit).toBe(666);
    expect(s.cEff).toBe(640);
    expect(zoomAt()).toBeCloseTo(1.25, 10);
    s = fitReducer(s, viewport(w, 832.5));
    expect(s.cEff).toBe(666);
    expect(s.transit).toBeNull();
    expect(zoomAt()).toBeCloseTo(1.25, 10);
  });
});

describe("zoomContentH", () => {
  it("is null for the initial state", () => {
    expect(zoomContentH(initialFitState)).toBeNull();
  });

  it("null after measured with no outcome", () => {
    expect(zoomContentH(run(initialFitState, measured(640)))).toBeNull();
  });

  it("null after a first deferred", () => {
    const s = run(initialFitState, measured(640), outcome(640, "deferred"));
    expect(s.cEff).toBeNull();
    expect(s.transit).toBeNull();
    expect(zoomContentH(s)).toBeNull();
  });

  it("is the minimum of the non-null heights once cEff or transit is set", () => {
    expect(zoomContentH(state({ cEff: 640, lastMeasured: 700 }))).toBe(640);
    expect(zoomContentH(state({ cEff: 900, lastMeasured: 640 }))).toBe(640);
    expect(zoomContentH(state({ cEff: 640, transit: 900, lastMeasured: 900 }))).toBe(640);
    expect(zoomContentH(state({ transit: 900, lastMeasured: 900 }))).toBe(900);
    expect(zoomContentH(state({ cEff: 640 }))).toBe(640);
  });
});

describe("shouldResend", () => {
  it("is true after deferred and each skipped outcome", () => {
    for (const o of ["deferred", "skippedMaximized", "skippedMinimized"] as const) {
      expect(shouldResend(run(initialFitState, measured(640), outcome(640, o)))).toBe(true);
    }
  });

  it("is false after alreadyFitted, applied, capped and failed", () => {
    for (const o of ["alreadyFitted", "applied", "capped"] as const) {
      const start = state({ retry: true });
      expect(shouldResend(run(start, measured(640), outcome(640, o)))).toBe(false);
    }
    const failed = run(state({ retry: true }), measured(640), { kind: "failed", c: 640 });
    expect(shouldResend(failed)).toBe(false);
  });

  it("is false without a measurement", () => {
    expect(shouldResend(state({ retry: true }))).toBe(false);
  });
});

describe("no zoom flash on grow or shrink, in either arrival order", () => {
  const w = 1225;
  type Case = { name: string; from: number; to: number; outcomeFirst: boolean };
  const cases: Case[] = [
    { name: "grow, outcome before the resize event", from: 640, to: 900, outcomeFirst: true },
    { name: "grow, resize event before the outcome", from: 640, to: 900, outcomeFirst: false },
    { name: "shrink, outcome before the resize event", from: 900, to: 640, outcomeFirst: true },
    { name: "shrink, resize event before the outcome", from: 900, to: 640, outcomeFirst: false },
  ];
  it.each(cases)("$name", ({ from, to, outcomeFirst }) => {
    let s = run(
      initialFitState,
      viewport(w, from * 1.25),
      measured(from),
      outcome(from, "alreadyFitted"),
    );
    expect(s.cEff).toBe(from);
    const zoomAt = (): number => windowZoom(w, s.viewport?.h ?? 0, zoomContentH(s));
    expect(zoomAt()).toBeCloseTo(1.25, 10);

    s = fitReducer(s, measured(to));
    expect(zoomAt()).toBeCloseTo(1.25, 10);

    const resize = viewport(w, to * 1.25);
    const reply = outcome(to, "applied");
    s = fitReducer(s, outcomeFirst ? reply : resize);
    expect(zoomAt()).toBeCloseTo(1.25, 10);
    s = fitReducer(s, outcomeFirst ? resize : reply);
    expect(zoomAt()).toBeCloseTo(1.25, 10);

    expect(s.cEff).toBe(to);
    expect(s.transit).toBeNull();
  });
});

describe("createFitController", () => {
  type Pending = { c: number; resolve: (o: FitOutcome) => void; reject: (e: unknown) => void };

  function setup() {
    const pending: Pending[] = [];
    const send = vi.fn((c: number) =>
      new Promise<FitOutcome>((resolve, reject) => {
        pending.push({ c, resolve, reject });
      }),
    );
    const published: FitState[] = [];
    const publish = vi.fn((s: FitState) => {
      published.push(s);
    });
    const warn = vi.fn();
    const controller = createFitController({ send, publish, warn });
    const last = (): FitState => {
      const s = published[published.length - 1];
      if (s === undefined) throw new Error("nothing published");
      return s;
    };
    const flush = async (): Promise<void> => {
      await vi.advanceTimersByTimeAsync(0);
    };
    return { controller, send, publish, warn, pending, published, last, flush };
  }

  const vp = { width: 1225, height: 800 };

  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("onMeasured publishes measured, sends c, then publishes the outcome for c", async () => {
    const t = setup();
    t.controller.onMeasured(640);
    expect(t.publish).toHaveBeenCalledTimes(1);
    expect(t.last().lastMeasured).toBe(640);
    expect(t.send).toHaveBeenCalledWith(640);
    t.pending[0]?.resolve("alreadyFitted");
    await t.flush();
    expect(t.publish).toHaveBeenCalledTimes(2);
    expect(t.last().cEff).toBe(640);
    expect(t.last().transit).toBeNull();
  });

  it("a rejection warns once and publishes failed for c", async () => {
    const t = setup();
    const boom = new Error("boom");
    t.controller.onMeasured(640);
    t.pending[0]?.reject(boom);
    await t.flush();
    expect(t.warn).toHaveBeenCalledTimes(1);
    expect(t.warn).toHaveBeenCalledWith("content height: fit failed", boom);
    expect(t.last().cEff).toBe(640);
    expect(t.last().retry).toBe(false);
  });

  it("a stale resolution is ignored", async () => {
    const t = setup();
    t.controller.onMeasured(640);
    t.controller.onMeasured(700);
    t.pending[0]?.resolve("applied");
    await t.flush();
    expect(t.last().transit).toBeNull();
    expect(t.last().cEff).toBeNull();
    expect(t.last().lastMeasured).toBe(700);
  });

  it("a viewport event with retry resends lastMeasured after 200 ms without a measured event", async () => {
    const t = setup();
    t.controller.onMeasured(640);
    t.pending[0]?.resolve("deferred");
    await t.flush();
    t.controller.onViewport(vp);
    const publishedBefore = t.publish.mock.calls.length;
    await vi.advanceTimersByTimeAsync(RESEND_DEBOUNCE_MS - 1);
    expect(t.send).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(t.send).toHaveBeenCalledTimes(2);
    expect(t.send).toHaveBeenLastCalledWith(640);
    expect(t.publish.mock.calls.length).toBe(publishedBefore);
  });

  it("N viewport events while deferred produce one resend", async () => {
    const t = setup();
    t.controller.onMeasured(640);
    t.pending[0]?.resolve("deferred");
    await t.flush();
    for (let i = 0; i < 20; i += 1) {
      t.controller.onViewport({ width: 1225, height: 800 + i });
      await vi.advanceTimersByTimeAsync(10);
    }
    await vi.advanceTimersByTimeAsync(RESEND_DEBOUNCE_MS);
    expect(t.send).toHaveBeenCalledTimes(2);
  });

  it("no resend while a call is in flight", async () => {
    const t = setup();
    t.controller.onMeasured(640);
    t.pending[0]?.resolve("deferred");
    await t.flush();
    t.controller.onViewport(vp);
    await vi.advanceTimersByTimeAsync(RESEND_DEBOUNCE_MS);
    expect(t.send).toHaveBeenCalledTimes(2);
    t.controller.onViewport({ width: 1225, height: 801 });
    await vi.advanceTimersByTimeAsync(RESEND_DEBOUNCE_MS);
    expect(t.send).toHaveBeenCalledTimes(2);
    t.pending[1]?.resolve("deferred");
    await t.flush();
    await vi.advanceTimersByTimeAsync(RESEND_DEBOUNCE_MS);
    expect(t.send).toHaveBeenCalledTimes(3);
  });

  it("no resend without retry", async () => {
    const t = setup();
    t.controller.onMeasured(640);
    t.pending[0]?.resolve("alreadyFitted");
    await t.flush();
    t.controller.onViewport(vp);
    t.controller.onViewport({ width: 1225, height: 801 });
    await vi.advanceTimersByTimeAsync(1000);
    expect(t.send).toHaveBeenCalledTimes(1);
  });

  it("a timer that fires after retry cleared sends nothing", async () => {
    const t = setup();
    t.controller.onMeasured(640);
    t.pending[0]?.resolve("deferred");
    await t.flush();
    t.controller.onViewport(vp);
    await vi.advanceTimersByTimeAsync(50);
    t.controller.onMeasured(700);
    t.pending[1]?.resolve("alreadyFitted");
    await t.flush();
    expect(t.last().retry).toBe(false);
    await vi.advanceTimersByTimeAsync(RESEND_DEBOUNCE_MS);
    expect(t.send).toHaveBeenCalledTimes(2);
  });

  it("dispose cancels the timer and drops later resolutions", async () => {
    const t = setup();
    t.controller.onMeasured(640);
    t.pending[0]?.resolve("deferred");
    await t.flush();
    t.controller.onViewport(vp);
    t.controller.onMeasured(700);
    const publishedBefore = t.publish.mock.calls.length;
    t.controller.dispose();
    t.pending[1]?.resolve("applied");
    await t.flush();
    await vi.advanceTimersByTimeAsync(1000);
    expect(t.send).toHaveBeenCalledTimes(2);
    expect(t.publish.mock.calls.length).toBe(publishedBefore);
    t.controller.onMeasured(800);
    t.controller.onViewport(vp);
    expect(t.send).toHaveBeenCalledTimes(2);
    expect(t.publish.mock.calls.length).toBe(publishedBefore);
  });

  it("dispose drops a later rejection without warning", async () => {
    const t = setup();
    t.controller.onMeasured(640);
    t.controller.dispose();
    t.pending[0]?.reject(new Error("late"));
    await t.flush();
    expect(t.warn).not.toHaveBeenCalled();
  });
});
