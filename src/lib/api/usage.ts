import { invoke } from "@tauri-apps/api/core";
import type {
  UsageSummary,
  UsageSummaryByApp,
  DailyStats,
  ProviderStats,
  ModelStats,
  RequestLog,
  LogFilters,
  ModelPricing,
  ModelsDevSyncConfig,
  ModelsDevSyncState,
  ProviderLimitStatus,
  PaginatedLogs,
  SessionSyncResult,
  DataSourceSummary,
  HermesUsageMetadata,
} from "@/types/usage";
import type { UsageResult } from "@/types";
import type { AppId } from "./types";
import type { TemplateType } from "@/config/constants";

export interface HermesRequestEvent {
  eventId: string;
  profileName: string;
  kind: string;
  sessionId: string;
  taskId: string;
  auxTask: string;
  model: string;
  provider: string;
  startedAtMs: number;
  endedAtMs: number;
  status: string;
  statusCode: number | null;
  durationMs: number | null;
  usageAvailable: boolean;
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  cacheWriteTokens: number | null;
  reasoningTokens: number | null;
}

export interface HermesHistoryEstimate {
  profileName: string;
  capturedAtMs: number;
  requestCount: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  reasoningTokens: number;
  costUsd: string;
}

export interface HermesReplayResult {
  imported: number;
  skipped: number;
  unavailable: number;
  errors: string[];
}

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
    providerName?: string,
    model?: string,
    profileName?: string,
    task?: string,
  ): Promise<UsageSummary> => {
    return invoke("get_usage_summary", {
      startDate,
      endDate,
      appType,
      providerName,
      model,
      profileName,
      task,
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
    providerName?: string,
    model?: string,
    profileName?: string,
    task?: string,
  ): Promise<UsageSummaryByApp[]> => {
    return invoke("get_usage_summary_by_app", {
      startDate,
      endDate,
      providerName,
      model,
      profileName,
      task,
    });
  },

  getUsageTrends: async (
    startDate?: number,
    endDate?: number,
    appType?: string,
    providerName?: string,
    model?: string,
    profileName?: string,
    task?: string,
  ): Promise<DailyStats[]> => {
    return invoke("get_usage_trends", {
      startDate,
      endDate,
      appType,
      providerName,
      model,
      profileName,
      task,
    });
  },

  getProviderStats: async (
    startDate?: number,
    endDate?: number,
    appType?: string,
    providerName?: string,
    model?: string,
    profileName?: string,
    task?: string,
  ): Promise<ProviderStats[]> => {
    return invoke("get_provider_stats", {
      startDate,
      endDate,
      appType,
      providerName,
      model,
      profileName,
      task,
    });
  },

  getModelStats: async (
    startDate?: number,
    endDate?: number,
    appType?: string,
    providerName?: string,
    model?: string,
    profileName?: string,
    task?: string,
  ): Promise<ModelStats[]> => {
    return invoke("get_model_stats", {
      startDate,
      endDate,
      appType,
      providerName,
      model,
      profileName,
      task,
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

  getHermesUsageMetadata: async (): Promise<HermesUsageMetadata> => {
    return invoke("get_hermes_usage_metadata");
  },

  getHermesRequestEvents: async (
    filters: {
      startMs?: number;
      endMs?: number;
      profileName?: string;
      task?: string;
      providerName?: string;
      model?: string;
      offset?: number;
    } = {},
  ): Promise<HermesRequestEvent[]> => {
    return invoke("get_hermes_request_events", filters);
  },

  getHermesHistoryEstimates: async (): Promise<HermesHistoryEstimate[]> => {
    return invoke("get_hermes_history_estimates");
  },

  replayHermesHistory: async (): Promise<HermesReplayResult> => {
    return invoke("replay_hermes_history");
  },

  enableHermesCapturePlugin: async (profileName?: string): Promise<string> => {
    return invoke("enable_hermes_capture_plugin", { profileName });
  },
};
