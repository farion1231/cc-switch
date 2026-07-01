import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

describe("settingsApi", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("keeps string pickDirectory calls compatible", async () => {
    invokeMock.mockResolvedValueOnce("/picked");
    const { settingsApi } = await import("@/lib/api/settings");

    await expect(settingsApi.pickDirectory("/workspace")).resolves.toBe(
      "/picked",
    );

    expect(invokeMock).toHaveBeenCalledWith("pick_directory", {
      defaultPath: "/workspace",
      title: undefined,
    });
  });

  it("passes the dialog title for object pickDirectory calls", async () => {
    invokeMock.mockResolvedValueOnce("/picked");
    const { settingsApi } = await import("@/lib/api/settings");

    await expect(
      settingsApi.pickDirectory({
        defaultPath: "/workspace",
        title: "Choose terminal working directory",
      }),
    ).resolves.toBe("/picked");

    expect(invokeMock).toHaveBeenCalledWith("pick_directory", {
      defaultPath: "/workspace",
      title: "Choose terminal working directory",
    });
  });
});
