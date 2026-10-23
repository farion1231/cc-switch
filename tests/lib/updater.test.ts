import { beforeEach, describe, expect, it, vi } from "vitest";

const getVersion = vi.fn(async () => "");
const updaterCheck = vi.fn();

vi.mock("@tauri-apps/api/app", () => ({ getVersion }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: updaterCheck }));

describe("official updater", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    updaterCheck.mockResolvedValue(null);
  });

  it("queries the official updater for DSH builds too", async () => {
    getVersion.mockResolvedValueOnce("3.20.3-dsh.2");
    const { checkForUpdate } = await import("../../src/lib/updater");

    await expect(checkForUpdate()).resolves.toEqual({
      status: "up-to-date",
    });
    expect(updaterCheck).toHaveBeenCalledWith({ timeout: 30000 });
  });

  it("returns official update metadata and forwards the timeout", async () => {
    getVersion.mockResolvedValueOnce("3.20.3-dsh.2");
    updaterCheck.mockResolvedValueOnce({
      version: "4.0.15",
      notes: "Release notes",
      date: "2026-10-19",
    });
    const { checkForUpdate } = await import("../../src/lib/updater");

    await expect(checkForUpdate({ timeout: 5000 })).resolves.toEqual({
      status: "available",
      info: {
        currentVersion: "3.20.3-dsh.2",
        availableVersion: "4.0.15",
        notes: "Release notes",
        pubDate: "2026-10-19",
      },
    });
    expect(updaterCheck).toHaveBeenCalledWith({ timeout: 5000 });
  });
});
