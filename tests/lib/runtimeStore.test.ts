import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

import {
  createRuntimeTransition,
  getRuntimeSnapshot,
  setRuntimeSnapshot,
} from "@/lib/runtime/store";
import {
  appInvoke,
  localInvoke,
  RuntimeInvokeError,
} from "@/lib/runtime/invoke";

describe("runtime-aware invocation", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    setRuntimeSnapshot({
      status: "local",
      generation: 0,
    });
  });

  it("keeps existing Tauri command in local mode", async () => {
    invokeMock.mockResolvedValue({ local: true });

    const result = await appInvoke(
      "get_providers",
      { app: "claude" },
      { remoteCommand: "provider.list" },
    );

    expect(result).toEqual({ local: true });
    expect(invokeMock).toHaveBeenCalledWith("get_providers", { app: "claude" });
  });

  it("wraps business command when remote runtime is online", async () => {
    setRuntimeSnapshot({
      status: "online",
      generation: 3,
      activeTargetId: "prod",
    });
    invokeMock.mockResolvedValue({ remote: true });

    const result = await appInvoke(
      "get_providers",
      { app: "codex" },
      { remoteCommand: "provider.list" },
    );

    expect(result).toEqual({ remote: true });
    expect(invokeMock).toHaveBeenCalledWith("remote_invoke", {
      command: "provider.list",
      args: { app: "codex" },
      generation: 3,
    });
  });

  it("rejects remote calls while offline and never falls back locally", async () => {
    setRuntimeSnapshot({
      status: "offline",
      generation: 4,
      activeTargetId: "prod",
      errorCode: "REMOTE_UNREACHABLE",
    });

    await expect(
      appInvoke(
        "get_providers",
        { app: "codex" },
        { remoteCommand: "provider.list" },
      ),
    ).rejects.toMatchObject({
      code: "REMOTE_OFFLINE",
    } satisfies Partial<RuntimeInvokeError>);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("always executes connection management locally", async () => {
    setRuntimeSnapshot({
      status: "online",
      generation: 2,
      activeTargetId: "prod",
    });
    invokeMock.mockResolvedValue([{ id: "prod" }]);

    await localInvoke("remote_list_targets");

    expect(invokeMock).toHaveBeenCalledWith("remote_list_targets", undefined);
    expect(getRuntimeSnapshot().activeTargetId).toBe("prod");
  });

  it("blocks business calls until switching back to local is acknowledged", async () => {
    const previous = {
      status: "online" as const,
      generation: 8,
      activeTargetId: "prod",
    };
    setRuntimeSnapshot(createRuntimeTransition(previous, undefined));

    expect(getRuntimeSnapshot()).toMatchObject({
      status: "connecting",
      generation: 9,
      activeTargetId: undefined,
    });
    await expect(
      appInvoke(
        "get_providers",
        { app: "codex" },
        { remoteCommand: "provider.list" },
      ),
    ).rejects.toMatchObject({ code: "REMOTE_OFFLINE" });
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
