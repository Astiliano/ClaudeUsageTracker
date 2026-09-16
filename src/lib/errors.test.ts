import { describe, expect, it } from "vitest";
import { errorMessage } from "./errors";

describe("errorMessage", () => {
  it("unwraps a serialized AppError", () => {
    expect(errorMessage({ code: "out_of_range", message: "interval_secs must be 10..=3600, got 5" }))
      .toBe("interval_secs must be 10..=3600, got 5");
  });
  it("unwraps an Error", () => {
    expect(errorMessage(new Error("boom"))).toBe("boom");
  });
  it("stringifies anything else", () => {
    expect(errorMessage("plain")).toBe("plain");
    expect(errorMessage(42)).toBe("42");
    expect(errorMessage({ nope: true })).toBe("[object Object]");
  });
});
