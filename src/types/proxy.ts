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
  grokbuild: boolean;
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
  // 每个应用目录保留的最大 request-log 会话文件数；0 表示关闭记录
  requestLogMaxSessions: number;
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

// request-log 会话文件元信息
export interface ProxyRequestLogFileMeta {
  appType: string;
  fileName: string;
  sizeBytes: number;
  modifiedAtMs: number;
  // 复用 session_manager 解析出的会话标题（匹配不到为空）
  sessionTitle?: string | null;
}

// request-log 记录分页结果（轻量行：列表展示字段 + token 数，不含 body）
export interface ProxyRequestLogRecordsPage {
  records: ProxyRequestLogListRow[];
  total: number;
}

// 列表行（get_proxy_request_log_records 返回；body 详情按 lineNo 单独取）
export interface ProxyRequestLogListRow {
  lineNo: number;
  startTime?: string | null;
  endTime?: string | null;
  method?: string | null;
  endpoint?: string | null;
  model?: string | null;
  durationMs?: number | null;
  isStreaming?: boolean;
  statusCode?: number | null;
  error?: string | null;
  promptTokens?: number | null;
  completionTokens?: number | null;
}

// 单条 request-log 记录（get_proxy_request_log_record 返回，含完整 body）
export interface ProxyRequestLogRecord {
  lineNo?: number;
  startTime?: string;
  endTime?: string;
  requestId?: string;
  sessionId?: string;
  providerId?: string;
  appType?: string;
  method?: string;
  endpoint?: string;
  model?: string;
  durationMs?: number;
  isStreaming?: boolean;
  requestHeaders?: { name: string; value: string }[];
  requestBody?: unknown;
  responseHeaders?: { name: string; value: string }[];
  responseBody?: unknown;
  statusCode?: number;
  error?: string | null;
}
