import { invoke } from "@tauri-apps/api/core";
import type {
  UsageSummary,
  UsageSummaryByApp,
  DailyStats,
  ProviderStats,
  ModelStats,
  ProjectStats,
  ProjectOption,
  DataQualityStats,
  RequestLog,
  LogFilters,
  ModelPricing,
  ModelsDevSyncConfig,
  ModelsDevSyncState,
  ProviderLimitStatus,
  PaginatedLogs,
  SessionSyncResult,
  DataSourceSummary,
} from "@/types/usage";
import type { UsageResult } from "@/types";
import type { AppId } from "./types";
import type { TemplateType } from "@/config/constants";

export const usageApi = {
  // Provider usage script methods
  query: async (providerId: string, appId: AppId): Promise<UsageResult> => {
    return invoke("queryProviderUsage", { providerId, app: appId });
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
    return invoke("testUsageScript", {
      providerId,
      app: appId,
      scriptCode,
      timeout,
      apiKey,
      baseUrl,
      accessToken,
      userId,
      templateType,
    });
  },

  // Proxy usage statistics methods
  getUsageSummary: async (
    startDate?: number,
    endDate?: number,
    appType?: string,
    providerAppType?: string,
    providerId?: string,
    providerName?: string,
    model?: string,
    projectKeys?: string[],
  ): Promise<UsageSummary> => {
    return invoke("get_usage_summary", {
      startDate,
      endDate,
      appType,
      providerAppType,
      providerId,
      providerName,
      model,
      projectKeys,
    });
  },

  getSessionUsageSummary: async (
    appType: string,
    sessionId: string,
  ): Promise<UsageSummary> => {
    return invoke("get_session_usage_summary", { appType, sessionId });
  },

  getUsageSummaryByApp: async (
    startDate?: number,
    endDate?: number,
    providerAppType?: string,
    providerId?: string,
    providerName?: string,
    model?: string,
    projectKeys?: string[],
  ): Promise<UsageSummaryByApp[]> => {
    return invoke("get_usage_summary_by_app", {
      startDate,
      endDate,
      providerAppType,
      providerId,
      providerName,
      model,
      projectKeys,
    });
  },

  getUsageTrends: async (
    startDate?: number,
    endDate?: number,
    appType?: string,
    providerAppType?: string,
    providerId?: string,
    providerName?: string,
    model?: string,
    projectKeys?: string[],
  ): Promise<DailyStats[]> => {
    return invoke("get_usage_trends", {
      startDate,
      endDate,
      appType,
      providerAppType,
      providerId,
      providerName,
      model,
      projectKeys,
    });
  },

  /** 最早有用量记录的本地日期 `YYYY-MM-DD`，没有记录时为 null */
  getUsageFirstDate: async (
    appType?: string,
    providerAppType?: string,
    providerId?: string,
    providerName?: string,
    model?: string,
    projectKeys?: string[],
  ): Promise<string | null> => {
    return invoke("get_usage_first_date", {
      appType,
      providerAppType,
      providerId,
      providerName,
      model,
      projectKeys,
    });
  },

  getProviderStats: async (
    startDate?: number,
    endDate?: number,
    appType?: string,
    providerAppType?: string,
    providerId?: string,
    providerName?: string,
    model?: string,
    projectKeys?: string[],
  ): Promise<ProviderStats[]> => {
    return invoke("get_provider_stats", {
      startDate,
      endDate,
      appType,
      providerAppType,
      providerId,
      providerName,
      model,
      projectKeys,
    });
  },

  getModelStats: async (
    startDate?: number,
    endDate?: number,
    appType?: string,
    providerAppType?: string,
    providerId?: string,
    providerName?: string,
    model?: string,
    projectKeys?: string[],
  ): Promise<ModelStats[]> => {
    return invoke("get_model_stats", {
      startDate,
      endDate,
      appType,
      providerAppType,
      providerId,
      providerName,
      model,
      projectKeys,
    });
  },

  getProjectStats: async (
    startDate?: number,
    endDate?: number,
    appType?: string,
    providerAppType?: string,
    providerId?: string,
    providerName?: string,
    model?: string,
    projectKeys?: string[],
  ): Promise<ProjectStats[]> => {
    return invoke("get_project_stats", {
      startDate,
      endDate,
      appType,
      providerAppType,
      providerId,
      providerName,
      model,
      projectKeys,
    });
  },

  getProjectProviderStats: async (
    projectKey: string,
    startDate?: number,
    endDate?: number,
    appType?: string,
  ): Promise<ProviderStats[]> => {
    return invoke("get_project_provider_stats", {
      projectKey,
      startDate,
      endDate,
      appType,
    });
  },

  getUsageProjects: async (
    startDate?: number,
    endDate?: number,
    appType?: string,
    providerAppType?: string,
    providerId?: string,
    providerName?: string,
    model?: string,
  ): Promise<ProjectOption[]> => {
    return invoke("get_usage_projects", {
      startDate,
      endDate,
      appType,
      providerAppType,
      providerId,
      providerName,
      model,
    });
  },

  getUsageDataQuality: async (
    startDate?: number,
    endDate?: number,
    appType?: string,
    providerAppType?: string,
    providerId?: string,
    providerName?: string,
    model?: string,
    projectKeys?: string[],
  ): Promise<DataQualityStats> => {
    return invoke("get_usage_data_quality", {
      startDate,
      endDate,
      appType,
      providerAppType,
      providerId,
      providerName,
      model,
      projectKeys,
    });
  },

  getRequestLogs: async (
    filters: LogFilters,
    page: number = 0,
    pageSize: number = 20,
  ): Promise<PaginatedLogs> => {
    return invoke("get_request_logs", {
      filters,
      page,
      pageSize,
    });
  },

  getRequestDetail: async (requestId: string): Promise<RequestLog | null> => {
    return invoke("get_request_detail", { requestId });
  },

  getModelPricing: async (): Promise<ModelPricing[]> => {
    return invoke("get_model_pricing");
  },

  updateModelPricing: async (
    modelId: string,
    displayName: string,
    inputCost: string,
    outputCost: string,
    cacheReadCost: string,
    cacheCreationCost: string,
  ): Promise<void> => {
    return invoke("update_model_pricing", {
      modelId,
      displayName,
      inputCost,
      outputCost,
      cacheReadCost,
      cacheCreationCost,
    });
  },

  updateModelPricingBatch: async (entries: ModelPricing[]): Promise<number> => {
    return invoke("update_model_pricing_batch", { entries });
  },

  getModelsDevSyncConfig: async (): Promise<ModelsDevSyncState> => {
    return invoke("get_models_dev_sync_config");
  },

  saveModelsDevSyncConfig: async (
    config: ModelsDevSyncConfig,
  ): Promise<void> => {
    return invoke("save_models_dev_sync_config", { config });
  },

  recordModelsDevSyncResult: async (
    syncedAt: number | null,
    error: string | null,
  ): Promise<void> => {
    return invoke("record_models_dev_sync_result", { syncedAt, error });
  },

  deleteModelPricing: async (modelId: string): Promise<void> => {
    return invoke("delete_model_pricing", { modelId });
  },

  checkProviderLimits: async (
    providerId: string,
    appType: string,
  ): Promise<ProviderLimitStatus> => {
    return invoke("check_provider_limits", { providerId, appType });
  },

  // Session usage sync
  syncSessionUsage: async (): Promise<SessionSyncResult> => {
    return invoke("sync_session_usage");
  },

  /** 会话日志扫描（后台定时或手动同步）最近一次完成的时间（毫秒）；本次启动后还没扫过时为 null */
  getSessionUsageLastSync: async (): Promise<number | null> => {
    return invoke("get_session_usage_last_sync");
  },

  rebuildCodexUsage: async (): Promise<SessionSyncResult> => {
    return invoke("rebuild_codex_usage");
  },

  getDataSourceBreakdown: async (): Promise<DataSourceSummary[]> => {
    return invoke("get_usage_data_sources");
  },
};
