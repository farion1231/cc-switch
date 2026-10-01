import { beforeEach, describe, expect, it, vi } from "vitest";
import { SKILLS_APP_IDS } from "@/config/appConfig";
import { emptyVpsApps, vpsApi, type VpsServer } from "@/lib/api/vps";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const host: VpsServer = {
  id: "00000000-0000-4000-8000-000000000001",
  name: "Test",
  purpose: "",
  host: "192.0.2.10",
  port: 22,
  user: "deploy",
  identityFile: "C:/keys/测试 key",
  apps: {
    ...emptyVpsApps(),
    claude: true,
    openclaw: true,
    "claude-desktop": true,
  },
};

describe("VPS API contract", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue([]);
  });

  it("sends only the existing Skill-supported apps on saves and connection tests", async () => {
    await vpsApi.saveServer(host);
    await vpsApi.testConnection(host, "request-id");
    for (const [, args] of invoke.mock.calls) {
      expect(Object.keys(args.server.apps).sort()).toEqual(
        [...SKILLS_APP_IDS].sort(),
      );
      expect(args.server.apps.claude).toBe(true);
      expect(args.server.apps.openclaw).toBeUndefined();
      expect(args.server.apps["claude-desktop"]).toBeUndefined();
      expect(args.server.identityFile).toBe(host.identityFile);
    }
    expect(invoke.mock.calls[0][0]).toBe("save_vps_server");
    expect(invoke.mock.calls[1][0]).toBe("test_vps_connection");
    expect(invoke.mock.calls[1][1].requestId).toBe("request-id");
    expect(host.apps.openclaw).toBe(true);
  });

  it("keeps password IPC separate from persisted server metadata", async () => {
    const server = {
      ...host,
      authMethod: "password",
      password: "must-not-be-persisted",
    } as VpsServer;
    await vpsApi.saveServer(server, "  exact password  ");
    await vpsApi.testConnection(server, "request-id", "  exact password  ");
    for (const [, args] of invoke.mock.calls) {
      expect(args.password).toBe("  exact password  ");
      expect(args.server.authMethod).toBe("password");
      expect(args.server).not.toHaveProperty("password");
    }
  });

  it("loads and deletes global hosts without an activeApp parameter", async () => {
    await vpsApi.getServers();
    await vpsApi.deleteServer(host.id);
    expect(invoke).toHaveBeenNthCalledWith(1, "get_vps_servers");
    expect(invoke).toHaveBeenNthCalledWith(2, "delete_vps_server", {
      id: host.id,
    });
  });

  it("confirms only a backend challenge token and cancels only a request ID", async () => {
    await vpsApi.confirmHostKey("opaque-token");
    await vpsApi.cancelConnectionTest("request-id");
    expect(invoke).toHaveBeenNthCalledWith(1, "confirm_vps_host_key", {
      confirmationToken: "opaque-token",
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "cancel_vps_connection_test", {
      requestId: "request-id",
    });
  });
});
