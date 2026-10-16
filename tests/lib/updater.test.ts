import { beforeEach, describe, expect, it, vi } from "vitest";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { checkForUpdate } from "@/lib/updater";

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: async () => "4.0.13",
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));

describe("checkForUpdate", () => {
  beforeEach(() => {
    vi.mocked(check).mockReset();
  });

  it("passes the plugin body through as notes", async () => {
    const body = JSON.stringify({ version: "4.0.14", items: [] });
    vi.mocked(check).mockResolvedValue({
      version: "4.0.14",
      body,
      date: "2026-10-16T00:00:00Z",
    } as Update);

    await expect(checkForUpdate()).resolves.toEqual({
      status: "available",
      info: {
        currentVersion: "4.0.13",
        availableVersion: "4.0.14",
        notes: body,
        pubDate: "2026-10-16T00:00:00Z",
      },
    });
    expect(check).toHaveBeenCalledWith({ timeout: 30000 });
  });

  it("reports no update when the plugin returns null", async () => {
    vi.mocked(check).mockResolvedValue(null);

    await expect(checkForUpdate()).resolves.toEqual({ status: "up-to-date" });
  });
});
