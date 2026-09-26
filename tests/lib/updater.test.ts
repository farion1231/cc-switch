import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: () => Promise.resolve("3.20.2"),
}));

import { checkForUpdate, type UpdateInfo } from "@/lib/updater";

describe("checkForUpdate", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("asks the backend command with the timeout instead of the plugin's JS client", async () => {
    invokeMock.mockResolvedValue(null);

    const result = await checkForUpdate({ timeout: 5000 });

    expect(invokeMock).toHaveBeenCalledWith("check_app_update", {
      timeoutMs: 5000,
    });
    expect(result).toEqual({ status: "up-to-date" });
  });

  it("maps the backend payload into UpdateInfo", async () => {
    invokeMock.mockResolvedValue({
      version: "3.21.0",
      notes: "release notes",
      pubDate: "2026-09-07 14:25:28.0 +00:00:00",
    });

    const result = await checkForUpdate();

    expect(invokeMock).toHaveBeenCalledWith("check_app_update", {
      timeoutMs: 30000,
    });
    expect(result).toEqual({
      status: "available",
      info: {
        currentVersion: "3.20.2",
        availableVersion: "3.21.0",
        notes: "release notes",
        pubDate: "2026-09-07 14:25:28.0 +00:00:00",
      },
    });
  });

  it("maps absent notes and pubDate to undefined rather than null", async () => {
    invokeMock.mockResolvedValue({
      version: "3.21.0",
      notes: null,
      pubDate: null,
    });

    const result = await checkForUpdate();

    expect(result.status).toBe("available");
    const { info } = result as { status: "available"; info: UpdateInfo };
    expect(info.availableVersion).toBe("3.21.0");
    // toEqual 会忽略值为 undefined 的键，断言 null 没漏到前端只能逐字段查
    expect(info.notes).toBeUndefined();
    expect(info.pubDate).toBeUndefined();
  });
});
