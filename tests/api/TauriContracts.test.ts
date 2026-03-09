import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

const { proxyApi } = await import("@/lib/api/proxy");
const { skillsApi } = await import("@/lib/api/skills");
const { providersApi } = await import("@/lib/api/providers");
const { usageApi } = await import("@/lib/api/usage");
const { settingsApi } = await import("@/lib/api/settings");
const { workspaceApi } = await import("@/lib/api/workspace");
const { deeplinkApi } = await import("@/lib/api/deeplink");
const { sessionsApi } = await import("@/lib/api/sessions");
const { openclawApi } = await import("@/lib/api/openclaw");
const { getGlobalProxyUrl, setGlobalProxyUrl } = await import(
  "@/lib/api/globalProxy"
);
const { checkEnvConflicts, deleteEnvVars, restoreEnvBackup } = await import(
  "@/lib/api/env"
);

describe("Tauri command contracts", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it("uses migrated proxy command names and payloads", async () => {
    invokeMock.mockResolvedValueOnce({
      running: false,
      address: "127.0.0.1",
      port: 15721,
    });
    await proxyApi.getProxyStatus();
    expect(invokeMock).toHaveBeenNthCalledWith(1, "get_proxy_status");

    await proxyApi.setProxyTakeoverForApp("claude", true);
    expect(invokeMock).toHaveBeenNthCalledWith(
      2,
      "set_proxy_takeover_for_app",
      { appType: "claude", enabled: true },
    );

    await proxyApi.switchProxyProvider("claude", "provider-a");
    expect(invokeMock).toHaveBeenNthCalledWith(
      3,
      "switch_proxy_provider",
      { appType: "claude", providerId: "provider-a" },
    );
  });

  it("uses migrated skill command names and payloads", async () => {
    invokeMock.mockResolvedValueOnce([]);
    await skillsApi.scanUnmanaged();
    expect(invokeMock).toHaveBeenNthCalledWith(1, "scan_unmanaged_skills");

    invokeMock.mockResolvedValueOnce([]);
    await skillsApi.installFromZip("/tmp/skill.zip", "claude");
    expect(invokeMock).toHaveBeenNthCalledWith(
      2,
      "install_skills_from_zip",
      { filePath: "/tmp/skill.zip", currentApp: "claude" },
    );

    invokeMock.mockResolvedValueOnce(true);
    await skillsApi.toggleApp("local:demo-skill", "claude", false);
    expect(invokeMock).toHaveBeenNthCalledWith(
      3,
      "toggle_skill_app",
      { id: "local:demo-skill", app: "claude", enabled: false },
    );
  });

  it("keeps provider live-import command contracts stable", async () => {
    invokeMock.mockResolvedValueOnce(1);
    await providersApi.importOpenCodeFromLive();
    expect(invokeMock).toHaveBeenNthCalledWith(
      1,
      "import_opencode_providers_from_live",
    );

    invokeMock.mockResolvedValueOnce(1);
    await providersApi.importOpenClawFromLive();
    expect(invokeMock).toHaveBeenNthCalledWith(
      2,
      "import_openclaw_providers_from_live",
    );
  });

  it("keeps usage and settings command contracts stable", async () => {
    invokeMock.mockResolvedValueOnce({
      totalRequests: 1,
      totalTokens: 2,
      totalCost: "0.01",
      modelBreakdown: [],
    });
    await usageApi.getUsageSummary(100, 200);
    expect(invokeMock).toHaveBeenNthCalledWith(1, "get_usage_summary", {
      startDate: 100,
      endDate: 200,
    });

    invokeMock.mockResolvedValueOnce([]);
    await usageApi.getRequestLogs({ appType: "claude" } as any, 2, 50);
    expect(invokeMock).toHaveBeenNthCalledWith(2, "get_request_logs", {
      filters: { appType: "claude" },
      page: 2,
      pageSize: 50,
    });

    invokeMock.mockResolvedValueOnce(undefined);
    await settingsApi.webdavSyncDownload();
    expect(invokeMock).toHaveBeenNthCalledWith(3, "webdav_sync_download");

    invokeMock.mockResolvedValueOnce(undefined);
    await settingsApi.setAutoLaunch(true);
    expect(invokeMock).toHaveBeenNthCalledWith(4, "set_auto_launch", {
      enabled: true,
    });
  });

  it("keeps workspace, deeplink, session and host command contracts stable", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await workspaceApi.writeDailyMemoryFile("2026-03-09.md", "hello");
    expect(invokeMock).toHaveBeenNthCalledWith(1, "write_daily_memory_file", {
      filename: "2026-03-09.md",
      content: "hello",
    });

    invokeMock.mockResolvedValueOnce({ version: "1", resource: "provider" });
    await deeplinkApi.parseDeeplink("ccswitch://v1/provider?app=claude");
    expect(invokeMock).toHaveBeenNthCalledWith(2, "parse_deeplink", {
      url: "ccswitch://v1/provider?app=claude",
    });

    invokeMock.mockResolvedValueOnce([]);
    await sessionsApi.list();
    expect(invokeMock).toHaveBeenNthCalledWith(3, "list_sessions");

    invokeMock.mockResolvedValueOnce(false);
    await openclawApi.getDefaultModel();
    expect(invokeMock).toHaveBeenNthCalledWith(4, "get_openclaw_default_model");
  });

  it("keeps env and global proxy command contracts stable", async () => {
    invokeMock.mockResolvedValueOnce(null);
    await getGlobalProxyUrl();
    expect(invokeMock).toHaveBeenNthCalledWith(1, "get_global_proxy_url");

    invokeMock.mockResolvedValueOnce(undefined);
    await setGlobalProxyUrl("http://127.0.0.1:7890");
    expect(invokeMock).toHaveBeenNthCalledWith(2, "set_global_proxy_url", {
      url: "http://127.0.0.1:7890",
    });

    invokeMock.mockResolvedValueOnce([]);
    await checkEnvConflicts("claude");
    expect(invokeMock).toHaveBeenNthCalledWith(3, "check_env_conflicts", {
      app: "claude",
    });

    invokeMock.mockResolvedValueOnce({ backupPath: "/tmp/env.backup" });
    await deleteEnvVars([{ app: "claude", filePath: "/tmp/.zshrc" }] as any);
    expect(invokeMock).toHaveBeenNthCalledWith(4, "delete_env_vars", {
      conflicts: [{ app: "claude", filePath: "/tmp/.zshrc" }],
    });

    invokeMock.mockResolvedValueOnce(undefined);
    await restoreEnvBackup("/tmp/env.backup");
    expect(invokeMock).toHaveBeenNthCalledWith(5, "restore_env_backup", {
      backupPath: "/tmp/env.backup",
    });
  });
});
