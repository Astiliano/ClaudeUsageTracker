import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(),
}));

import { backend } from "./backend";

describe("backend().setContentHeight", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue("applied");
  });

  it("setContentHeight invokes set_content_height with the camelCase key", async () => {
    await expect(backend().setContentHeight(640)).resolves.toBe("applied");
    expect(invoke).toHaveBeenCalledWith("set_content_height", { contentH: 640 });
  });
});
