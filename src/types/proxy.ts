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

/** 应用当前的连接方式。 */
export type AppMode = "direct" | "route" | "stack";

/** 应用页模式行用的状态（后端 `get_app_mode`）。 */
export interface AppModeView {
  mode: AppMode;
  /** 客户端文件指着代理（CC Switch 运行时为真） */
  attached: boolean;
  /** 路由目标；直连模式下是上次路由的那家 */
  routeProviderId: string | null;
  /** 直连那家：直连时写进客户端的、回到直连时写回的 */
  directProviderId: string | null;
}

/** 启动时没能接上代理、已退回直连的应用。 */
export interface StartupAttachFailure {
  appType: string;
  stack: boolean;
  error: string;
}

/** Stack 模型：名单里的一家和它发布给客户端的模型 id。 */
export interface ProxyStackMember {
  providerId: string;
  modelIds: string[];
  /** 这家是默认那家（代理路由）：模型走默认路由，`modelIds` 等默认换到别家后才发布。 */
  route: boolean;
}

/**
 * Codex Stack 模型客户端看不到或看不全：`routeOwnsCatalog` 路由供应商使用自己的模型目录文件，
 * Stack 模型不发布；官方做路由时官方模型列表暂未取到：`officialModelsBundled` 暂用 Codex
 * 自带的列表（可能缺账号专属的模型），`officialModelsUnavailable` Stack 模型暂不可用。
 */
export type ProxyStackNotice =
  | "routeOwnsCatalog"
  | "officialModelsBundled"
  | "officialModelsUnavailable";

/**
 * 还在用旧模型列表的 Codex 客户端（它们只在启动时读模型目录）：`daemon` 是 `codex` 命令行连的
 * 托管守护进程，可以一键重启；`others` 是桌面版、编辑器插件，要用户自己彻底退出再开。进出
 * 路由 / 聚合、增删 Stack 模型、退出路由都会换模型目录，这些之后都可能出现。
 */
export interface CodexStaleClients {
  daemon: boolean;
  others: boolean;
}

/** Stack 模式的状态、名单和提示。 */
export interface ProxyStack {
  /** 在 Stack 模式（代理模式且 Stack 模式开着）。 */
  active: boolean;
  members: ProxyStackMember[];
  notice?: ProxyStackNotice;
  staleClients?: CodexStaleClients;
}

/**
 * Claude Code 的场景绑定（#7889）：某个场景指向哪个已发布的 Stack 模型 id；空 / 缺省 = 跟随
 * 默认行为（四档用默认那家列表里的第一个模型、子代理跟随主模型、辅助 / 压缩请求不分流）。
 */
export interface ClaudeStackScenarios {
  haiku?: string | null;
  sonnet?: string | null;
  opus?: string | null;
  fable?: string | null;
  /** 子代理（`CLAUDE_CODE_SUBAGENT_MODEL`）。 */
  subagent?: string | null;
  /** 辅助请求，含 Auto Mode 分类器（请求头 `x-claude-code-request-class: auxiliary`）。 */
  auxiliary?: string | null;
  /** 上下文压缩请求（同头 `compaction`）。 */
  compaction?: string | null;
}

/** 能绑定到场景的 Stack 模型。 */
export interface ClaudeScenarioModel {
  id: string;
  /** 选择器里显示的名字：`<显示名>（<供应商名>）`。 */
  label: string;
}

/** Claude Code 的场景绑定和能绑定的模型。 */
export interface ClaudeScenarioView {
  /** 当前绑定，原样：对不上 `models` 的是失效的绑定。 */
  scenarios: ClaudeStackScenarios;
  /** 能选的模型：已发布给客户端的 Stack 模型；空 = 还没有能绑的模型。 */
  models: ClaudeScenarioModel[];
}

/** 重启 Codex 守护进程的结果：`notRunning` 表示它没在运行，什么都没做。 */
export type CodexDaemonRestartOutcome = "restarted" | "notRunning";

/** 增删 Stack 模型失败。`partial` 为真：已部分写入，下次操作或重启 CC Switch 时补完。 */
export interface ProxyStackWriteError {
  partial: boolean;
  message: string;
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
