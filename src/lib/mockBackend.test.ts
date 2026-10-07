import { describe, expect, it, vi } from "vitest";
import { createMockBackend } from "./mockBackend";
import type { UserSettings } from "./types";

async function settingsWith(
  b: ReturnType<typeof createMockBackend>,
  over: Partial<UserSettings>,
): Promise<UserSettings> {
  const current = await b.invoke<UserSettings>("get_settings");
  return { ...current, ...over };
}

describe("mock set_settings: min_free_memory_mb", () => {
  it("defaults to 1536 and round-trips a new floor", async () => {
    const b = createMockBackend("");
    expect((await b.invoke<UserSettings>("get_settings")).min_free_memory_mb).toBe(1536);
    await b.invoke("set_settings", { settings: await settingsWith(b, { min_free_memory_mb: 2048 }) });
    expect((await b.invoke<UserSettings>("get_settings")).min_free_memory_mb).toBe(2048);
  });

  it("accepts both ends of 0..=65536", async () => {
    const b = createMockBackend("");
    for (const mb of [0, 65536]) {
      await b.invoke("set_settings", { settings: await settingsWith(b, { min_free_memory_mb: mb }) });
      expect((await b.invoke<UserSettings>("get_settings")).min_free_memory_mb).toBe(mb);
    }
  });

  it("rejects 65537 with out_of_range and keeps the previous floor", async () => {
    const b = createMockBackend("");
    await expect(
      b.invoke("set_settings", { settings: await settingsWith(b, { min_free_memory_mb: 65537 }) }),
    ).rejects.toMatchObject({ code: "out_of_range" });
    expect((await b.invoke<UserSettings>("get_settings")).min_free_memory_mb).toBe(1536);
  });

  it("rejects a payload without the field", async () => {
    const b = createMockBackend("");
    const { min_free_memory_mb: _omitted, ...without } = await settingsWith(b, {});
    await expect(b.invoke("set_settings", { settings: without })).rejects.toBeDefined();
  });

  it("serves a dashboard with no memory hold", async () => {
    const b = createMockBackend("");
    const d = await b.invoke<{ memory_hold: unknown }>("get_dashboard");
    expect(d.memory_hold).toBeNull();
  });
});

describe("mock setContentHeight", () => {
  it("setContentHeight records calls and resolves alreadyFitted", async () => {
    const info = vi.spyOn(console, "info").mockImplementation(() => undefined);
    try {
      const b = createMockBackend("");
      await expect(b.setContentHeight(640)).resolves.toBe("alreadyFitted");
      expect(info).toHaveBeenCalledWith("mock: setContentHeight", 640);
    } finally {
      info.mockRestore();
    }
  });
});
