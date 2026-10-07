import { afterEach, describe, expect, it, vi } from "vitest";
import {
  attachContentHeight,
  subscribeContentHeight,
  type MeasuredElement,
} from "./contentHeight";

type Entry = { borderBoxSize: readonly { blockSize: number }[] };

/** Captures its callback and records construction, `observe` and `disconnect`. */
class FakeObserver {
  static constructed = 0;
  static last: FakeObserver | null = null;
  readonly observed: { el: MeasuredElement; opts: { box: "border-box" } }[] = [];
  disconnects = 0;
  constructor(readonly cb: (entries: readonly Entry[]) => void) {
    FakeObserver.constructed += 1;
    FakeObserver.last = this;
  }
  observe(el: MeasuredElement, opts: { box: "border-box" }): void {
    this.observed.push({ el, opts });
  }
  disconnect(): void {
    this.disconnects += 1;
  }
  fire(...sizes: number[]): void {
    this.cb(sizes.map((blockSize) => ({ borderBoxSize: [{ blockSize }] })));
  }
}

function fresh(): FakeObserver {
  const o = FakeObserver.last;
  if (o === null) throw new Error("no observer constructed");
  return o;
}

const element: MeasuredElement = { offsetHeight: 0 };

afterEach(() => {
  FakeObserver.constructed = 0;
  FakeObserver.last = null;
  vi.restoreAllMocks();
});

describe("subscribeContentHeight", () => {
  it("first report comes from the observer's initial callback", () => {
    const reports: number[] = [];
    subscribeContentHeight(element, FakeObserver, (h) => reports.push(h));
    expect(FakeObserver.constructed).toBe(1);
    expect(fresh().observed).toEqual([{ el: element, opts: { box: "border-box" } }]);
    expect(reports).toEqual([]);
    fresh().fire(212);
    expect(reports).toEqual([212]);
  });

  it("reports the ceiling of the border box less one layout unit", () => {
    const reports: number[] = [];
    subscribeContentHeight(element, FakeObserver, (h) => reports.push(h));
    fresh().fire(200.2);
    fresh().fire(640);
    fresh().fire(640.004);
    expect(reports).toEqual([201, 640]);
  });

  it("dedupes against the last value reported", () => {
    const reports: number[] = [];
    subscribeContentHeight(element, FakeObserver, (h) => reports.push(h));
    fresh().fire(212);
    fresh().fire(211.6);
    fresh().fire(212);
    expect(reports).toEqual([212]);
    fresh().fire(300);
    expect(reports).toEqual([212, 300]);
  });

  it("ignores non-finite and non-positive sizes", () => {
    const reports: number[] = [];
    subscribeContentHeight(element, FakeObserver, (h) => reports.push(h));
    fresh().fire(0);
    fresh().fire(Number.NaN);
    fresh().fire(Number.POSITIVE_INFINITY);
    expect(reports).toEqual([]);
  });

  it("reads the last entry's borderBoxSize", () => {
    const reports: number[] = [];
    subscribeContentHeight(element, FakeObserver, (h) => reports.push(h));
    fresh().fire(100, 250);
    expect(reports).toEqual([250]);
  });

  it("unsubscribe disconnects and later callbacks report nothing", () => {
    const reports: number[] = [];
    const off = subscribeContentHeight(element, FakeObserver, (h) => reports.push(h));
    fresh().fire(212);
    off();
    expect(fresh().disconnects).toBe(1);
    fresh().fire(500);
    expect(reports).toEqual([212]);
  });

  it("no ResizeObserver: warns once and reports offsetHeight once", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const reports: number[] = [];
    const off = subscribeContentHeight({ offsetHeight: 212.4 }, undefined, (h) => reports.push(h));
    expect(reports).toEqual([213]);
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn).toHaveBeenCalledWith("content height: ResizeObserver unavailable; reporting once");
    expect(() => off()).not.toThrow();
  });

  it("no ResizeObserver: a zero offsetHeight reports nothing", () => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const reports: number[] = [];
    subscribeContentHeight({ offsetHeight: 0 }, undefined, (h) => reports.push(h));
    expect(reports).toEqual([]);
  });
});

describe("attachContentHeight", () => {
  it("disabled or detached constructs no observer", () => {
    const reports: number[] = [];
    const report = (h: number): void => {
      reports.push(h);
    };
    const offDisabled = attachContentHeight(false, element, FakeObserver, report);
    const offDetached = attachContentHeight(true, null, FakeObserver, report);
    expect(FakeObserver.constructed).toBe(0);
    expect(reports).toEqual([]);
    expect(() => offDisabled()).not.toThrow();
    expect(() => offDetached()).not.toThrow();
  });

  it("enabled subscribes", () => {
    const reports: number[] = [];
    const off = attachContentHeight(true, element, FakeObserver, (h) => reports.push(h));
    expect(FakeObserver.constructed).toBe(1);
    fresh().fire(212);
    expect(reports).toEqual([212]);
    off();
    expect(fresh().disconnects).toBe(1);
  });
});
