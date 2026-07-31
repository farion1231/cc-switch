import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

import { settingsApi } from "./settings";

describe("settingsApi managed targets", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("lists targets and inspects a registered target by stable id", async () => {
    invokeMock
      .mockResolvedValueOnce([{ id: "windows-codex" }])
      .mockResolvedValueOnce({
        targetId: "windows-codex",
        reachable: true,
      })
      .mockResolvedValueOnce([
        { distro: "Ubuntu", user: "mikasa", reachable: true },
      ])
      .mockResolvedValueOnce({ id: "wsl-ubuntu-mikasa" })
      .mockResolvedValueOnce({
        id: "wsl-ubuntu-mikasa",
        currentProviderId: "provider-a",
        managementState: "unmanaged",
      })
      .mockResolvedValueOnce({
        id: "wsl-ubuntu-mikasa",
        currentProviderId: "provider-a",
        managementState: "managed",
      })
      .mockResolvedValueOnce({
        id: "wsl-ubuntu-mikasa",
        currentProviderId: "provider-b",
        managementState: "managed",
      })
      .mockResolvedValueOnce({
        changedJsonlFiles: 48,
        changedStateRows: 48,
        backupPath: "/backup/migrate",
      })
      .mockResolvedValueOnce({
        changedJsonlFiles: 48,
        changedStateRows: 48,
        backupPath: "/backup/restore",
      });

    await expect(settingsApi.listManagedTargets()).resolves.toEqual([
      { id: "windows-codex" },
    ]);
    await expect(
      settingsApi.inspectManagedTarget("windows-codex"),
    ).resolves.toMatchObject({ reachable: true });
    await expect(settingsApi.discoverWslTargets()).resolves.toEqual([
      { distro: "Ubuntu", user: "mikasa", reachable: true },
    ]);
    await expect(
      settingsApi.registerDiscoveredWslTarget("Ubuntu"),
    ).resolves.toEqual({ id: "wsl-ubuntu-mikasa" });
    await expect(
      settingsApi.linkManagedTargetProvider("wsl-ubuntu-mikasa", "provider-a"),
    ).resolves.toMatchObject({
      currentProviderId: "provider-a",
      managementState: "unmanaged",
    });
    await expect(
      settingsApi.activateWslManagedTarget("wsl-ubuntu-mikasa"),
    ).resolves.toMatchObject({ managementState: "managed" });
    await expect(
      settingsApi.switchManagedTargetProvider(
        "wsl-ubuntu-mikasa",
        "provider-b",
      ),
    ).resolves.toMatchObject({ currentProviderId: "provider-b" });
    await expect(
      settingsApi.migrateManagedTargetCodexHistory("wsl-ubuntu-mikasa"),
    ).resolves.toMatchObject({ changedJsonlFiles: 48 });
    await expect(
      settingsApi.restoreManagedTargetCodexHistory("wsl-ubuntu-mikasa"),
    ).resolves.toMatchObject({ changedStateRows: 48 });

    expect(invokeMock).toHaveBeenNthCalledWith(1, "listManagedTargets");
    expect(invokeMock).toHaveBeenNthCalledWith(2, "inspectManagedTarget", {
      targetId: "windows-codex",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "discoverWslTargets");
    expect(invokeMock).toHaveBeenNthCalledWith(
      4,
      "registerDiscoveredWslTarget",
      { distro: "Ubuntu" },
    );
    expect(invokeMock).toHaveBeenNthCalledWith(5, "linkManagedTargetProvider", {
      targetId: "wsl-ubuntu-mikasa",
      providerId: "provider-a",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(6, "activateWslManagedTarget", {
      targetId: "wsl-ubuntu-mikasa",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(
      7,
      "switchManagedTargetProvider",
      {
        targetId: "wsl-ubuntu-mikasa",
        providerId: "provider-b",
      },
    );
    expect(invokeMock).toHaveBeenNthCalledWith(
      8,
      "migrateManagedTargetCodexHistory",
      { targetId: "wsl-ubuntu-mikasa" },
    );
    expect(invokeMock).toHaveBeenNthCalledWith(
      9,
      "restoreManagedTargetCodexHistory",
      { targetId: "wsl-ubuntu-mikasa" },
    );
  });
});
