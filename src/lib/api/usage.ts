import type {
  UsageSummary,
  UsageSummaryByApp,
  DailyStats,
  ProviderStats,
  ModelStats,
  RequestLog,
  LogFilters,
  ModelPricing,
  ProviderLimitStatus,
  PaginatedLogs,
  SessionSyncResult,
  DataSourceSummary,
} from "@/types/usage";
import type { UsageResult } from "@/types";
import type { AppId } from "./types";
import type { TemplateType } from "@/config/constants";
import { appInvoke } from "@/lib/runtime/invoke";

export const usageApi = {
  // 维护说明：每个 Usage 方法都必须显式声明 Agent RPC 名称；appInvoke 负责按当前 runtime
  // 选择主机并阻断过渡/离线状态，禁止任何远端失败后写回本机数据库的隐式回退。
  // Provider usage script methods
  query: async (providerId: string, appId: AppId): Promise<UsageResult> => {
    return appInvoke(
      "queryProviderUsage",
      { providerId, app: appId },
      { remoteCommand: "usage.provider_query" },
    );
  },

  testScript: async (
    providerId: string,
    appId: AppId,
    scriptCode: string,
    timeout?: number,
    apiKey?: string,
    baseUrl?: string,
    accessToken?: string,
    userId?: string,
    templateType?: TemplateType,
  ): Promise<UsageResult> => {
    return appInvoke(
      "testUsageScript",
      {
        providerId,
        app: appId,
        scriptCode,
        timeout,
        apiKey,
        baseUrl,
        accessToken,
        userId,
        templateType,
      },
      { remoteCommand: "usage.provider_test" },
    );
  },

  // Proxy usage statistics methods
  getUsageSummary: async (
    startDate?: number,
    endDate?: number,
    appType?: string,
    providerName?: string,
    model?: string,
  ): Promise<UsageSummary> => {
    return appInvoke(
      "get_usage_summary",
      { startDate, endDate, appType, providerName, model },
      { remoteCommand: "usage.summary" },
    );
  },

  getUsageSummaryByApp: async (
    startDate?: number,
    endDate?: number,
    providerName?: string,
    model?: string,
  ): Promise<UsageSummaryByApp[]> => {
    return appInvoke(
      "get_usage_summary_by_app",
      { startDate, endDate, providerName, model },
      { remoteCommand: "usage.summary_by_app" },
    );
  },

  getUsageTrends: async (
    startDate?: number,
    endDate?: number,
    appType?: string,
    providerName?: string,
    model?: string,
  ): Promise<DailyStats[]> => {
    return appInvoke(
      "get_usage_trends",
      { startDate, endDate, appType, providerName, model },
      { remoteCommand: "usage.trends" },
    );
  },

  getProviderStats: async (
    startDate?: number,
    endDate?: number,
    appType?: string,
    providerName?: string,
    model?: string,
  ): Promise<ProviderStats[]> => {
    return appInvoke(
      "get_provider_stats",
      { startDate, endDate, appType, providerName, model },
      { remoteCommand: "usage.provider_stats" },
    );
  },

  getModelStats: async (
    startDate?: number,
    endDate?: number,
    appType?: string,
    providerName?: string,
    model?: string,
  ): Promise<ModelStats[]> => {
    return appInvoke(
      "get_model_stats",
      { startDate, endDate, appType, providerName, model },
      { remoteCommand: "usage.model_stats" },
    );
  },

  getRequestLogs: async (
    filters: LogFilters,
    page: number = 0,
    pageSize: number = 20,
  ): Promise<PaginatedLogs> => {
    return appInvoke(
      "get_request_logs",
      { filters, page, pageSize },
      { remoteCommand: "usage.logs" },
    );
  },

  getRequestDetail: async (requestId: string): Promise<RequestLog | null> => {
    return appInvoke(
      "get_request_detail",
      { requestId },
      { remoteCommand: "usage.detail" },
    );
  },

  getModelPricing: async (): Promise<ModelPricing[]> => {
    return appInvoke("get_model_pricing", undefined, {
      remoteCommand: "usage.pricing.list",
    });
  },

  updateModelPricing: async (
    modelId: string,
    displayName: string,
    inputCost: string,
    outputCost: string,
    cacheReadCost: string,
    cacheCreationCost: string,
  ): Promise<void> => {
    return appInvoke(
      "update_model_pricing",
      {
        modelId,
        displayName,
        inputCost,
        outputCost,
        cacheReadCost,
        cacheCreationCost,
      },
      { remoteCommand: "usage.pricing.update" },
    );
  },

  deleteModelPricing: async (modelId: string): Promise<void> => {
    return appInvoke(
      "delete_model_pricing",
      { modelId },
      { remoteCommand: "usage.pricing.delete" },
    );
  },

  checkProviderLimits: async (
    providerId: string,
    appType: string,
  ): Promise<ProviderLimitStatus> => {
    return appInvoke(
      "check_provider_limits",
      { providerId, appType },
      { remoteCommand: "usage.limits" },
    );
  },

  // Session usage sync
  syncSessionUsage: async (): Promise<SessionSyncResult> => {
    return appInvoke("sync_session_usage", undefined, {
      remoteCommand: "usage.session_sync",
    });
  },

  rebuildCodexUsage: async (): Promise<SessionSyncResult> => {
    return appInvoke("rebuild_codex_usage", undefined, {
      remoteCommand: "usage.codex_rebuild",
    });
  },

  getDataSourceBreakdown: async (): Promise<DataSourceSummary[]> => {
    return appInvoke("get_usage_data_sources", undefined, {
      remoteCommand: "usage.data_sources",
    });
  },
};
