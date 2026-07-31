import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

import { usageApi } from "./usage";
import { setRuntimeSnapshot } from "@/lib/runtime/store";

type UsageCall = {
  name: string;
  localCommand: string;
  remoteCommand: string;
  args?: Record<string, unknown>;
  invoke: () => Promise<unknown>;
};

// 维护说明：这里故意完整枚举 Usage API，而不是只抽查几个代表方法；新增接口若漏掉远端映射，
// 就可能在远端模式下静默写入本机数据库，因此映射表本身也是跨主机隔离契约的一部分。
const usageCalls: UsageCall[] = [
  {
    name: "provider usage query",
    localCommand: "queryProviderUsage",
    remoteCommand: "usage.provider_query",
    args: { providerId: "provider-a", app: "codex" },
    invoke: () => usageApi.query("provider-a", "codex"),
  },
  {
    name: "provider usage script test",
    localCommand: "testUsageScript",
    remoteCommand: "usage.provider_test",
    args: {
      providerId: "provider-a",
      app: "claude",
      scriptCode: "return { used: 1 };",
      timeout: 5,
      apiKey: "test-key",
      baseUrl: "https://usage.example.test",
      accessToken: "test-token",
      userId: "user-a",
      templateType: "custom",
    },
    invoke: () =>
      usageApi.testScript(
        "provider-a",
        "claude",
        "return { used: 1 };",
        5,
        "test-key",
        "https://usage.example.test",
        "test-token",
        "user-a",
        "custom",
      ),
  },
  {
    name: "usage summary",
    localCommand: "get_usage_summary",
    remoteCommand: "usage.summary",
    args: {
      startDate: 100,
      endDate: 200,
      appType: "codex",
      providerName: "provider-a",
      model: "model-a",
    },
    invoke: () =>
      usageApi.getUsageSummary(100, 200, "codex", "provider-a", "model-a"),
  },
  {
    name: "usage summary by app",
    localCommand: "get_usage_summary_by_app",
    remoteCommand: "usage.summary_by_app",
    args: {
      startDate: 100,
      endDate: 200,
      providerName: "provider-a",
      model: "model-a",
    },
    invoke: () =>
      usageApi.getUsageSummaryByApp(100, 200, "provider-a", "model-a"),
  },
  {
    name: "usage trends",
    localCommand: "get_usage_trends",
    remoteCommand: "usage.trends",
    args: {
      startDate: 100,
      endDate: 200,
      appType: "codex",
      providerName: "provider-a",
      model: "model-a",
    },
    invoke: () =>
      usageApi.getUsageTrends(100, 200, "codex", "provider-a", "model-a"),
  },
  {
    name: "provider stats",
    localCommand: "get_provider_stats",
    remoteCommand: "usage.provider_stats",
    args: {
      startDate: 100,
      endDate: 200,
      appType: "codex",
      providerName: "provider-a",
      model: "model-a",
    },
    invoke: () =>
      usageApi.getProviderStats(100, 200, "codex", "provider-a", "model-a"),
  },
  {
    name: "model stats",
    localCommand: "get_model_stats",
    remoteCommand: "usage.model_stats",
    args: {
      startDate: 100,
      endDate: 200,
      appType: "codex",
      providerName: "provider-a",
      model: "model-a",
    },
    invoke: () =>
      usageApi.getModelStats(100, 200, "codex", "provider-a", "model-a"),
  },
  {
    name: "request logs",
    localCommand: "get_request_logs",
    remoteCommand: "usage.logs",
    args: {
      filters: { appType: "codex", providerName: "provider-a" },
      page: 2,
      pageSize: 50,
    },
    invoke: () =>
      usageApi.getRequestLogs(
        { appType: "codex", providerName: "provider-a" },
        2,
        50,
      ),
  },
  {
    name: "request detail",
    localCommand: "get_request_detail",
    remoteCommand: "usage.detail",
    args: { requestId: "request-a" },
    invoke: () => usageApi.getRequestDetail("request-a"),
  },
  {
    name: "model pricing list",
    localCommand: "get_model_pricing",
    remoteCommand: "usage.pricing.list",
    invoke: () => usageApi.getModelPricing(),
  },
  {
    name: "model pricing update",
    localCommand: "update_model_pricing",
    remoteCommand: "usage.pricing.update",
    args: {
      modelId: "model-a",
      displayName: "Model A",
      inputCost: "1",
      outputCost: "2",
      cacheReadCost: "0.1",
      cacheCreationCost: "0.2",
    },
    invoke: () =>
      usageApi.updateModelPricing("model-a", "Model A", "1", "2", "0.1", "0.2"),
  },
  {
    name: "model pricing batch update",
    localCommand: "update_model_pricing_batch",
    remoteCommand: "usage.pricing.update_batch",
    args: {
      entries: [
        {
          modelId: "model-a",
          displayName: "Model A",
          inputCostPerMillion: "1",
          outputCostPerMillion: "2",
          cacheReadCostPerMillion: "0.1",
          cacheCreationCostPerMillion: "0.2",
        },
      ],
    },
    invoke: () =>
      usageApi.updateModelPricingBatch([
        {
          modelId: "model-a",
          displayName: "Model A",
          inputCostPerMillion: "1",
          outputCostPerMillion: "2",
          cacheReadCostPerMillion: "0.1",
          cacheCreationCostPerMillion: "0.2",
        },
      ]),
  },
  {
    name: "models.dev sync config read",
    localCommand: "get_models_dev_sync_config",
    remoteCommand: "usage.models_dev_sync.get",
    invoke: () => usageApi.getModelsDevSyncConfig(),
  },
  {
    name: "models.dev sync config save",
    localCommand: "save_models_dev_sync_config",
    remoteCommand: "usage.models_dev_sync.save",
    args: {
      config: {
        autoSyncEnabled: true,
        includeCommonModels: false,
        selectedModelKeys: ["openai:gpt-5"],
        excludedCommonModelKeys: [],
        lastSyncAt: null,
        lastSyncError: null,
      },
    },
    invoke: () =>
      usageApi.saveModelsDevSyncConfig({
        autoSyncEnabled: true,
        includeCommonModels: false,
        selectedModelKeys: ["openai:gpt-5"],
        excludedCommonModelKeys: [],
        lastSyncAt: null,
        lastSyncError: null,
      }),
  },
  {
    name: "models.dev sync result record",
    localCommand: "record_models_dev_sync_result",
    remoteCommand: "usage.models_dev_sync.record",
    args: { syncedAt: 123, error: null },
    invoke: () => usageApi.recordModelsDevSyncResult(123, null),
  },
  {
    name: "model pricing delete",
    localCommand: "delete_model_pricing",
    remoteCommand: "usage.pricing.delete",
    args: { modelId: "model-a" },
    invoke: () => usageApi.deleteModelPricing("model-a"),
  },
  {
    name: "provider limits",
    localCommand: "check_provider_limits",
    remoteCommand: "usage.limits",
    args: { providerId: "provider-a", appType: "codex" },
    invoke: () => usageApi.checkProviderLimits("provider-a", "codex"),
  },
  {
    name: "session sync",
    localCommand: "sync_session_usage",
    remoteCommand: "usage.session_sync",
    invoke: () => usageApi.syncSessionUsage(),
  },
  {
    name: "Codex rebuild",
    localCommand: "rebuild_codex_usage",
    remoteCommand: "usage.codex_rebuild",
    invoke: () => usageApi.rebuildCodexUsage(),
  },
  {
    name: "data source breakdown",
    localCommand: "get_usage_data_sources",
    remoteCommand: "usage.data_sources",
    invoke: () => usageApi.getDataSourceBreakdown(),
  },
];

describe("runtime-aware Usage API", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
    setRuntimeSnapshot({ status: "local", generation: 0 });
  });

  it.each(usageCalls)(
    "keeps $name on the local Tauri command",
    async (call) => {
      await call.invoke();

      expect(invokeMock).toHaveBeenCalledWith(call.localCommand, call.args);
    },
  );

  it.each(usageCalls)(
    "routes $name to the online remote target",
    async (call) => {
      setRuntimeSnapshot({
        status: "online",
        generation: 9,
        activeTargetId: "server-a",
      });

      await call.invoke();

      expect(invokeMock).toHaveBeenCalledWith("remote_invoke", {
        command: call.remoteCommand,
        args: call.args ?? {},
        generation: 9,
      });
    },
  );

  it.each(["offline", "connecting"] as const)(
    "rejects Usage calls while runtime is %s without falling back locally",
    async (status) => {
      setRuntimeSnapshot({
        status,
        generation: 11,
        activeTargetId: "server-a",
      });

      await expect(usageApi.getUsageSummary()).rejects.toMatchObject({
        code: "REMOTE_OFFLINE",
      });
      expect(invokeMock).not.toHaveBeenCalled();
    },
  );
});
