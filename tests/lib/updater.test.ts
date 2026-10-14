import { describe, expect, it, vi } from "vitest";

const getVersion = vi.fn(async () => "");
const updaterCheck = vi.fn();

vi.mock("@tauri-apps/api/app", () => ({ getVersion }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: updaterCheck }));

describe("DSH updater", () => {
  it("does not query the official updater for DSH builds", async () => {
    getVersion.mockResolvedValueOnce("3.20.3-dsh.2");
    const { checkForUpdate } = await import("../../src/lib/updater");

    await expect(checkForUpdate()).resolves.toEqual({
      status: "up-to-date",
    });
    expect(updaterCheck).not.toHaveBeenCalled();
  });
});
