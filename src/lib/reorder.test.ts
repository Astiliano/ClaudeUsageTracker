import { describe, expect, it } from "vitest";
import { moveItem } from "./reorder";

describe("moveItem", () => {
  it("moves an item down (to a higher index)", () => {
    expect(moveItem(["a", "b", "c"], 0, 2)).toEqual(["b", "c", "a"]);
  });

  it("moves an item up (to a lower index)", () => {
    expect(moveItem(["a", "b", "c"], 2, 0)).toEqual(["c", "a", "b"]);
  });

  it("is a no-op when from and to are the same index", () => {
    const list = ["a", "b", "c"];
    expect(moveItem(list, 1, 1)).toEqual(["a", "b", "c"]);
  });

  it("returns a copy unchanged when from is out of range", () => {
    const list = ["a", "b", "c"];
    const result = moveItem(list, -1, 1);
    expect(result).toEqual(list);
    expect(result).not.toBe(list);
  });

  it("returns a copy unchanged when to is out of range", () => {
    const list = ["a", "b", "c"];
    const result = moveItem(list, 0, 5);
    expect(result).toEqual(list);
    expect(result).not.toBe(list);
  });

  it("does not mutate the input list", () => {
    const list = ["a", "b", "c"];
    moveItem(list, 0, 2);
    expect(list).toEqual(["a", "b", "c"]);
  });

  it("works on objects, keyed by reference not value", () => {
    const a = { id: "a" };
    const b = { id: "b" };
    const c = { id: "c" };
    expect(moveItem([a, b, c], 2, 0)).toEqual([c, a, b]);
  });
});
