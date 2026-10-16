import { beforeEach, describe, expect, it, vi } from "vitest";
import { check } from "@tauri-apps/plugin-updater";
import { checkForUpdate } from "@/lib/updater";

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("4.0.13"),
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));

describe("update metadata", () => {
  beforeEach(() => vi.mocked(check).mockReset());

  it("reads the plugin body as release notes and closes the resource without downloading", async () => {
    const update = {
      currentVersion: "4.0.13",
      version: "4.1.0",
      body: "## New features\n- Full release announcement",
      date: "2026-10-16T00:00:00Z",
      close: vi.fn().mockResolvedValue(undefined),
      download: vi.fn(),
      downloadAndInstall: vi.fn(),
    };
    vi.mocked(check).mockResolvedValue(update as never);

    await expect(checkForUpdate()).resolves.toEqual({
      status: "available",
      info: {
        currentVersion: "4.0.13",
        availableVersion: "4.1.0",
        notes: update.body,
        pubDate: update.date,
      },
    });
    expect(update.close).toHaveBeenCalledOnce();
    expect(update.download).not.toHaveBeenCalled();
    expect(update.downloadAndInstall).not.toHaveBeenCalled();
  });

  it("reports no update when the plugin returns null", async () => {
    vi.mocked(check).mockResolvedValue(null);
    await expect(checkForUpdate({ timeout: 5000 })).resolves.toEqual({
      status: "up-to-date",
    });
    expect(check).toHaveBeenCalledWith({ timeout: 5000 });
  });
});
