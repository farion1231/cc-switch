import type { SessionMessage } from "@/types";

export interface ProxyConfig {
  listen_address: string;
  listen_port: number;
  max_retries: number;
  request_timeout: number;
  enable_logging: boolean;
  live_takeover_active?: boolean;
  // 超时配置
  streaming_first_byte_timeout: number;
  streaming_idle_timeout: number;
  non_streaming_timeout: number;
}

export interface ProxyStatus {
  running: boolean;
  address: string;
  port: number;
  active_connections: number;
  total_requests: number;
  success_requests: number;
  failed_requests: number;
  success_rate: number;
  uptime_seconds: number;
  current_provider: string | null;
  current_provider_id: string | null;
  last_request_at: string | null;
  last_error: string | null;
  failover_count: number;
  active_targets?: ActiveTarget[];
}

export interface ActiveTarget {
  app_type: string;
  provider_name: string;
  provider_id: string;
}

export interface ProxyServerInfo {
  address: string;
  port: number;
  started_at: string;
}

export interface ProxyTakeoverStatus {
  claude: boolean;
  "claude-desktop"?: boolean;
  codex: boolean;
  gemini: boolean;
  opencode: boolean;
  openclaw: boolean;
  hermes: boolean;
}

export interface ProviderHealth {
  provider_id: string;
  app_type: string;
  is_healthy: boolean;
  consecutive_failures: number;
  last_success_at: string | null;
  last_failure_at: string | null;
  last_error: string | null;
  updated_at: string;
}

// 熔断器相关类型
export interface CircuitBreakerConfig {
  failureThreshold: number;
  successThreshold: number;
  timeoutSeconds: number;
  errorRateThreshold: number;
  minRequests: number;
}

export type CircuitState = "closed" | "open" | "half_open";

export interface CircuitBreakerStats {
  state: CircuitState;
  consecutiveFailures: number;
  consecutiveSuccesses: number;
  totalRequests: number;
  failedRequests: number;
}

// 供应商健康状态枚举
export enum ProviderHealthStatus {
  Healthy = "healthy",
  Degraded = "degraded",
  Failed = "failed",
  Unknown = "unknown",
}

// 扩展 ProviderHealth 以包含前端计算的状态
export interface ProviderHealthWithStatus extends ProviderHealth {
  status: ProviderHealthStatus;
  circuitState?: CircuitState;
}

export interface ProxyUsageRecord {
  provider_id: string;
  app_type: string;
  endpoint: string;
  request_tokens: number | null;
  response_tokens: number | null;
  status_code: number;
  latency_ms: number;
  error: string | null;
  timestamp: string;
}

// 故障转移队列条目
export interface FailoverQueueItem {
  providerId: string;
  providerName: string;
  providerNotes?: string;
  sortIndex?: number;
}

// 全局代理配置（统一字段，三行镜像）
export interface GlobalProxyConfig {
  proxyEnabled: boolean;
  listenAddress: string;
  listenPort: number;
  enableLogging: boolean;
}

// 应用级代理配置（每个 app 独立）
export interface AppProxyConfig {
  appType: string;
  enabled: boolean;
  autoFailoverEnabled: boolean;
  maxRetries: number;
  streamingFirstByteTimeout: number;
  streamingIdleTimeout: number;
  nonStreamingTimeout: number;
  circuitFailureThreshold: number;
  circuitSuccessThreshold: number;
  circuitTimeoutSeconds: number;
  circuitErrorRateThreshold: number;
  circuitMinRequests: number;
}

export interface CodexHistoryVisibilityRepairOptions {
  dryRun: boolean;
  codexHome?: string | null;
  stateDbPath?: string | null;
  projectPath?: string | null;
  sessionIds?: string[] | null;
  targetProvider?: string | null;
  count?: number | null;
  windowLimit?: number | null;
  balanceRecentWindow?: boolean | null;
  maxPerProject?: number | null;
  maxTotal?: number | null;
  sourceFilter?: string | null;
  includeArchived?: boolean | null;
  includeSubagents?: boolean | null;
  skipProviderBucketSync?: boolean | null;
}

export interface CodexHistoryVisibilityRepairOutcome {
  dryRun: boolean;
  codexHome: string;
  stateDbPath: string | null;
  activeDbKind: string | null;
  liveConfigModelProvider: string | null;
  targetProvider: string;
  sourceProviderIds: string[];
  sqliteThreads: number;
  providerRowsToUpdate: number;
  providerRowsUpdated: number;
  rolloutFirstLinesToUpdate: number;
  rolloutFirstLinesUpdated: number;
  userEventRowsToUpdate: number;
  userEventRowsUpdated: number;
  visibleCandidateRows: number;
  sessionIndexMissingToAppend: number;
  sessionIndexAppended: number;
  projectRows: number;
  focusSelectedCount: number;
  balancedRecentWindowEnabled: boolean;
  balancedRecentWindowRows: number;
  balancedRecentWindowProjects: number;
  maxPerProject: number;
  maxTotal: number;
  sourceFilter: string | null;
  sqliteFocusRowsToUpdate: number;
  sqliteFocusRowsUpdated: number;
  sessionIndexTitlesToUpdate: number;
  sessionIndexTitlesUpdated: number;
  sessionIndexRowsToMove: number;
  sessionIndexRowsMoved: number;
  workspaceHintsToFix: number;
  workspaceHintsFixed: number;
  projectlessIdsToRemove: number;
  projectlessIdsRemoved: number;
  savedWorkspaceRootsToAdd: number;
  savedWorkspaceRootsAdded: number;
  rolloutMtimesToTouch: number;
  rolloutMtimesTouched: number;
  visibleProjectRowsInWindowBefore: number;
  backupDir: string | null;
  skippedReason: string | null;
}

export interface CodexHistorySessionListOptions {
  codexHome?: string | null;
  stateDbPath?: string | null;
  projectPath?: string | null;
  provider?: string | null;
  sourceFilter?: string | null;
  query?: string | null;
  limit?: number | null;
  includeArchived?: boolean | null;
  includeSubagents?: boolean | null;
}

export interface CodexHistoryValueCount {
  value: string | null;
  count: number;
}

export interface CodexHistorySessionSummary {
  id: string;
  title: string;
  cwd: string | null;
  modelProvider: string | null;
  source: string | null;
  threadSource: string | null;
  archived: boolean;
  hasUserEvent: boolean;
  updatedAtMs: number;
  updatedAt: string | null;
  rolloutPath: string | null;
}

export interface CodexHistorySessionListOutcome {
  codexHome: string;
  stateDbPath: string | null;
  activeDbKind: string | null;
  liveConfigModelProvider: string | null;
  targetProviderCandidates: string[];
  sourceCounts: CodexHistoryValueCount[];
  providerCounts: CodexHistoryValueCount[];
  totalMatched: number;
  items: CodexHistorySessionSummary[];
  skippedReason: string | null;
}

export interface CodexHistorySessionDetailOptions {
  codexHome?: string | null;
  stateDbPath?: string | null;
  sessionId: string;
}

export interface CodexHistorySessionDetailOutcome {
  codexHome: string;
  stateDbPath: string | null;
  activeDbKind: string | null;
  session: CodexHistorySessionSummary | null;
  messages: SessionMessage[];
  rolloutPath: string | null;
  skippedReason: string | null;
}
