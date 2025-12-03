// 使用统计相关类型定义

export interface TokenUsage {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
}

export interface RequestLog {
  requestId: string;
  providerId: string;
  providerName?: string;
  appType: string;
  model: string;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  inputCostUsd: string;
  outputCostUsd: string;
  cacheReadCostUsd: string;
  cacheCreationCostUsd: string;
  totalCostUsd: string;
  isStreaming: boolean;
  latencyMs: number;
  firstTokenMs?: number;
  durationMs?: number;
  statusCode: number;
  errorMessage?: string;
  createdAt: number;
}

export interface PaginatedLogs {
  data: RequestLog[];
  total: number;
  page: number;
  pageSize: number;
}

export interface ModelPricing {
  modelId: string;
  displayName: string;
  inputCostPerMillion: string;
  outputCostPerMillion: string;
  cacheReadCostPerMillion: string;
  cacheCreationCostPerMillion: string;
}

export interface UsageSummary {
  totalRequests: number;
  totalCost: string;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheCreationTokens: number;
  totalCacheReadTokens: number;
  successRate: number;
}

export interface DailyStats {
  date: string;
  requestCount: number;
  totalCost: string;
  totalTokens: number;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheCreationTokens: number;
  totalCacheReadTokens: number;
}

export interface ProviderStats {
  providerId: string;
  providerName: string;
  requestCount: number;
  totalTokens: number;
  totalCost: string;
  successRate: number;
  avgLatencyMs: number;
}

export interface ModelStats {
  model: string;
  requestCount: number;
  totalTokens: number;
  totalCost: string;
  avgCostPerRequest: string;
}

export interface LogFilters {
  appType?: string;
  providerName?: string;
  model?: string;
  statusCode?: number;
  startDate?: number;
  endDate?: number;
}

export interface ProviderLimitStatus {
  providerId: string;
  dailyUsage: string;
  dailyLimit?: string;
  dailyExceeded: boolean;
  monthlyUsage: string;
  monthlyLimit?: string;
  monthlyExceeded: boolean;
}

export type TimeRange = "1d" | "7d" | "30d";

export interface StatsFilters {
  timeRange: TimeRange;
  providerId?: string;
  appType?: string;
}
