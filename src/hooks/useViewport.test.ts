import { describe, expect, it } from "vitest";
import { readViewport, subscribeViewport } from "./useViewport";
import type { Viewport, ViewportSource } from "./useViewport";

class FakeWindow implements ViewportSource {
  innerWidth = 980;
  innerHeight = 640;
  listeners: Array<() => void> = [];
  addEventListener(_type: "resize", listener: () => void): void {
    this.listeners.push(listener);
  }
  removeEventListener(_type: "resize", listener: () => void): void {
    this.listeners = this.listeners.filter((l) => l !== listener);
  }
  resize(width: number, height: number): void {
    this.innerWidth = width;
    this.innerHeight = height;
    for (const l of this.listeners) l();
  }
}

describe("subscribeViewport", () => {
  it("reports a height-only window resize through the resize event", () => {
    const win = new FakeWindow();
    const seen: Viewport[] = [];
    const stop = subscribeViewport(win, (v) => seen.push(v), null);
    win.resize(980, 900);
    expect(seen).toEqual([{ width: 980, height: 900 }]);
    stop();
    win.resize(980, 1000);
    expect(seen).toHaveLength(1);
    expect(win.listeners).toHaveLength(0);
  });

  it("observes the given element and disconnects on unsubscribe", () => {
    const win = new FakeWindow();
    const calls: string[] = [];
    let trigger: (() => void) | null = null;
    class FakeObserver {
      constructor(cb: () => void) {
        trigger = cb;
      }
      observe(): void {
        calls.push("observe");
      }
      disconnect(): void {
        calls.push("disconnect");
      }
      unobserve(): void {}
    }
    const seen: Viewport[] = [];
    const stop = subscribeViewport(win, (v) => seen.push(v), {
      element: {} as Element,
      observer: FakeObserver as unknown as typeof ResizeObserver,
    });
    win.innerWidth = 1200;
    (trigger as unknown as () => void)();
    expect(seen).toEqual([{ width: 1200, height: 640 }]);
    stop();
    expect(calls).toEqual(["observe", "disconnect"]);
  });

  it("readViewport returns the window's inner size", () => {
    expect(readViewport(new FakeWindow())).toEqual({ width: 980, height: 640 });
  });
});
