// 使用统计相关类型定义

import type { AppId } from "@/lib/api/types";

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
  requestModel?: string;
  /** 写入时实际用于计价的模型名；路由接管 + request 计价模式下可能与 model 不同 */
  pricingModel?: string;
  costMultiplier: string;
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
  dataSource?: string;
}

export interface SessionSyncResult {
  imported: number;
  skipped: number;
  filesScanned: number;
  suspectedDuplicates: number;
  deferredFiles: number;
  errors: string[];
}

/** Providers supported by the explicit historical session-usage rebuild. */
export const AGENT_USAGE_REBUILD_APPS = [
  "claude",
  "codex",
  "grokbuild",
  "opencode",
  "hermes",
  "pi",
] as const;

export type AgentUsageRebuildApp = (typeof AGENT_USAGE_REBUILD_APPS)[number];

export interface RebuildAgentSessionUsageRequest {
  appTypes: AgentUsageRebuildApp[];
}

export type ProviderUsageRebuildStatus = "published" | "keptPrevious";

export interface ProviderUsageRebuildResult {
  appType: AgentUsageRebuildApp;
  status: ProviderUsageRebuildStatus;
  syncResult: SessionSyncResult;
}

export interface RebuildAgentSessionUsageResult {
  providers: ProviderUsageRebuildResult[];
}

export interface DataSourceSummary {
  dataSource: string;
  requestCount: number;
  totalCostUsd: string;
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

export interface ModelsDevSyncConfig {
  autoSyncEnabled: boolean;
  includeCommonModels: boolean;
  selectedModelKeys: string[];
  excludedCommonModelKeys: string[];
  lastSyncAt: number | null;
  lastSyncError: string | null;
}

export interface ModelsDevSyncState {
  config: ModelsDevSyncConfig;
  configPath: string;
}

export interface UsageSummary {
  totalRequests: number;
  totalCost: string;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheCreationTokens: number;
  totalCacheReadTokens: number;
  successRate: number;
  /** input + output + cache_creation + cache_read, all cache-normalized */
  realTotalTokens: number;
  /** cache_read / (input + cache_creation + cache_read), range 0–1 */
  cacheHitRate: number;
}

export interface UsageSummaryByApp {
  appType: string;
  summary: UsageSummary;
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

/**
 * Dashboard 顶栏的全局筛选维度，作用于 Hero / 趋势图 / 三个统计 Tab。
 *
 * - `providerName` 按展示名精确匹配（与 Provider 统计列表同口径，含
 *   "Claude (Session)" 等会话占位名）；
 * - `model` 按「有效计价模型」匹配（pricing_model 优先、回落 model，
 *   与模型统计的分组口径一致）。
 */
export interface UsageScopeFilters {
  appType?: string;
  providerName?: string;
  model?: string;
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

export type UsageRangePreset = "today" | "1d" | "7d" | "14d" | "30d" | "custom";

export interface UsageRangeSelection {
  preset: UsageRangePreset;
  customStartDate?: number;
  customEndDate?: number;
  /** When true (custom mode only), endDate resolves to "now" instead of the
   *  fixed customEndDate snapshot, and the end-time field becomes read-only. */
  liveEndTime?: boolean;
}

/**
 * App types surfaced as dashboard filter buttons.
 *
 * `claude-desktop` is intentionally NOT listed: the Desktop gateway's proxy
 * traffic is still recorded under its own `app_type` (preserving route-takeover
 * billing audit — the request detail panel shows the real value), but the
 * dashboard folds it into `claude` for display. It is the embedded Claude Code
 * runtime running inside the Desktop shell, and Desktop *chat* usage never
 * passes through this app at all, so a separate "Claude Desktop" bucket would
 * only ever show a partial number and mislead users into reading it as the
 * Desktop's full usage. The backend collapses `claude-desktop → claude` in
 * every dashboard query (see `folded_app_type_sql`).
 * `opencode` and `pi` have no proxy handler; their usage reaches this
 * dashboard through session importers. `openclaw` / `hermes` appear only as
 * managed apps elsewhere.
 */
export type AppType =
  | "claude"
  | "codex"
  | "gemini"
  | "grokbuild"
  | "opencode"
  | "pi";

export type AppTypeFilter = "all" | AppType;

export const KNOWN_APP_TYPES: ReadonlyArray<AppType> = [
  "claude",
  "codex",
  "gemini",
  "grokbuild",
  "opencode",
  "pi",
];

/**
 * App types whose proxy uses an OpenAI-style protocol. Two consequences:
 *
 * 1. `inputTokens` already includes the cached portion (must subtract
 *    `cacheReadTokens` to get fresh-input semantics — see
 *    [getFreshInputTokens]).
 * 2. The protocol does not report cache _creation_ separately, only cache
 *    _reads_. So `cacheCreationTokens` is always 0 for these app types and
 *    the UI should label it as N/A rather than 0.
 *
 * Mirror of the Rust `CACHE_INCLUSIVE_APP_TYPES` whitelist.
 */
export const CACHE_INCLUSIVE_APP_TYPES: ReadonlySet<string> = new Set([
  "codex",
  "gemini",
  "grokbuild",
]);

// Pi sessions can mix Anthropic and OpenAI APIs, but the dashboard aggregates
// only by app type. Treat cache-write coverage as partial without changing
// Pi's fresh-input token semantics.
const PARTIAL_CACHE_WRITE_APP_TYPES: ReadonlySet<string> = new Set(["pi"]);

export type CacheWriteAvailability = "ok" | "partial" | "na";

export function getCacheWriteAvailability(
  appTypes: readonly string[],
): CacheWriteAvailability {
  if (appTypes.length === 0) return "ok";
  const unavailable = appTypes.filter((appType) =>
    CACHE_INCLUSIVE_APP_TYPES.has(appType),
  ).length;
  if (unavailable === appTypes.length) return "na";
  const partial = appTypes.some((appType) =>
    PARTIAL_CACHE_WRITE_APP_TYPES.has(appType),
  );
  return unavailable === 0 && !partial ? "ok" : "partial";
}

/** Subset of request-log fields needed to derive cache-normalized input. */
export interface CacheNormalizableLog {
  appType: string;
  inputTokens: number;
  cacheReadTokens: number;
}

/**
 * For a single request log, return the input token count with cache reads
 * removed. Anthropic-style providers already report `inputTokens` without
 * cache, so they pass through unchanged.
 */
export function getFreshInputTokens(log: CacheNormalizableLog): number {
  if (
    CACHE_INCLUSIVE_APP_TYPES.has(log.appType) &&
    log.inputTokens >= log.cacheReadTokens
  ) {
    return log.inputTokens - log.cacheReadTokens;
  }
  return log.inputTokens;
}

export const NON_NEGATIVE_DECIMAL_REGEX = /^\d+(?:\.\d+)?$/;

export function isNonNegativeDecimalString(value: string): boolean {
  const trimmed = value.trim();
  if (!NON_NEGATIVE_DECIMAL_REGEX.test(trimmed)) return false;
  return Number.isFinite(Number(trimmed));
}

type UsageCostLog = Pick<
  RequestLog,
  | "inputTokens"
  | "outputTokens"
  | "cacheReadTokens"
  | "cacheCreationTokens"
  | "totalCostUsd"
  | "statusCode"
> &
  Partial<Pick<RequestLog, "costMultiplier">>;

export function hasUsageTokens(log: UsageCostLog): boolean {
  return (
    log.inputTokens > 0 ||
    log.outputTokens > 0 ||
    log.cacheReadTokens > 0 ||
    log.cacheCreationTokens > 0
  );
}

export function isUnpricedUsage(log: UsageCostLog): boolean {
  const totalCost = Number.parseFloat(log.totalCostUsd);
  const multiplier =
    log.costMultiplier == null
      ? undefined
      : Number.parseFloat(log.costMultiplier);
  return (
    log.statusCode >= 200 &&
    log.statusCode < 300 &&
    hasUsageTokens(log) &&
    Number.isFinite(totalCost) &&
    (!Number.isFinite(multiplier) || multiplier !== 0) &&
    totalCost === 0
  );
}

export interface StatsFilters {
  timeRange: UsageRangePreset;
  providerId?: string;
  appType?: string;
}

// ============================================================================
// Canonical Agent session/task usage contract
// ============================================================================

/**
 * The backend capability registry is authoritative for runtime support.  This
 * alias deliberately reuses the app identifier union used by the rest of the
 * frontend so a newly managed app cannot silently become an untyped usage
 * bucket.
 */
export type AgentUsageAppType = AppId;

export type AgentUsagePrecision =
  | "request_exact"
  | "session_exact"
  | "sync_window_delta"
  | "estimated"
  | "unavailable";

export type AgentUsageTimeSemantics =
  | "event_time"
  | "session_time"
  | "sync_window_end"
  | "unavailable";

/**
 * Counts are source events, not automatically HTTP requests.  In particular,
 * Codex/Grok agent calls, Claude/Gemini assistant messages, and Pi usage
 * carriers remain distinct.
 */
export type AgentUsageRequestCountSemantics =
  | "http_request"
  | "assistant_message"
  | "agent_call"
  | "usage_event"
  | "unavailable";

export type AgentDescendantUsageStatus =
  | "available"
  | "no_activity_in_range"
  | "unavailable"
  | "not_applicable";

export type AgentUsageCapabilityStatus =
  | "supported"
  | "partial"
  | "unavailable";

export type AgentSessionNodeKind =
  | "root"
  | "child"
  | "standalone"
  | "unknown"
  | "conflict";

export type AgentSessionRelationConfidence =
  | "explicit"
  | "structural"
  | "unavailable"
  | "conflict";

/** Inclusive Unix-second range accepted by the Tauri commands. */
export interface AgentUsageRange {
  startAt?: number;
  endAt?: number;
}

/** Input envelope for `get_agent_session_usage`. */
export interface AgentSessionUsageRequest {
  appType: AgentUsageAppType;
  sessionId: string;
  range?: AgentUsageRange | null;
}

/** Input envelope for `list_agent_task_usage`. */
export interface AgentTaskUsageFilter {
  appType?: AgentUsageAppType;
  title?: string;
  project?: string;
  projectDir?: string;
  /** Exact native title selected from the task-statistics combobox. */
  titleExact?: string;
  /** Exact native project directory selected from the task-statistics combobox. */
  projectDirExact?: string;
  range?: AgentUsageRange | null;
  limit?: number;
  offset?: number;
}

/** Scope for the complete task title/project candidate list. */
export interface AgentTaskUsageFilterOptionsRequest {
  appType?: AgentUsageAppType;
  range?: AgentUsageRange | null;
}

export interface AgentTaskUsageProjectOption {
  projectDir: string;
}

export interface AgentTaskUsageFilterOptions {
  titles: string[];
  projects: AgentTaskUsageProjectOption[];
}

/** A normalized measure returned by the backend query layer. */
export interface AgentUsageMeasure {
  dataSource: string | null;
  requestCount: number | null;
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  cacheCreationTokens: number | null;
  totalCostUsd: string | null;
  precision: AgentUsagePrecision;
  timeSemantics: AgentUsageTimeSemantics;
  requestCountSemantics: AgentUsageRequestCountSemantics;
  partial: boolean;
  warnings: string[];
}

// DTO-shaped aliases keep the backend names discoverable to callers while
// retaining the explicit Agent prefix used by this file's public types.
export type UsageMeasure = AgentUsageMeasure;
export type UsagePrecision = AgentUsagePrecision;
export type TimeSemantics = AgentUsageTimeSemantics;
export type RequestCountSemantics = AgentUsageRequestCountSemantics;
export type CapabilityStatus = AgentUsageCapabilityStatus;

export interface AgentSessionNodeView {
  appType: AgentUsageAppType;
  sessionId: string;
  parentSessionId: string | null;
  rootSessionId: string;
  nodeKind: AgentSessionNodeKind;
  relationConfidence: AgentSessionRelationConfidence;
  title: string | null;
  projectDir: string | null;
  sourcePath: string | null;
  createdAt: number | null;
  lastActiveAt: number | null;
  lastSyncedAt: number;
}

export interface AgentUsageSourceDimension {
  providerId: string;
  model: string;
  requestModel: string;
  pricingModel: string;
  dataSource: string;
  inputTokenSemantics: number;
  sourceIdentity: string;
  profileId: string;
  databaseIdentity: string;
  baseUrlDigest: string;
  billingMode: string;
  task: string;
  sourceVersion: string;
  syncWindowStart: number;
  syncWindowEnd: number;
  apiCallCount: number | null;
  cacheWriteTokens: number | null;
  reasoningTokens: number | null;
  costStatus: string | null;
  costSource: string | null;
  costDeltaKind: string | null;
  correctionState: string | null;
  rangePartial: boolean;
}

export interface AgentUsageCapability {
  appType: AgentUsageAppType;
  sessionEnumeration: AgentUsageCapabilityStatus;
  usageStatus: AgentUsageCapabilityStatus;
  supportsDescendants: boolean;
  tokenStatus: AgentUsageCapabilityStatus;
  costStatus: AgentUsageCapabilityStatus;
  precision: AgentUsagePrecision;
  timeSemantics: AgentUsageTimeSemantics;
  requestCountSemantics: AgentUsageRequestCountSemantics;
  notes: string;
}

/** Compile-time guard that a capability map has one entry per managed AppId. */
export type AgentUsageCapabilityByApp = {
  [App in AgentUsageAppType]: AgentUsageCapability & { appType: App };
};

export interface AgentSessionUsageSummary {
  appType: AgentUsageAppType;
  requestedSessionId: string;
  sessionId: string;
  rootSessionId: string;
  rootResolved: boolean;
  root: AgentSessionNodeView | null;
  supportsDescendants: boolean;
  selfUsage: AgentUsageMeasure | null;
  descendantUsage: AgentUsageMeasure | null;
  descendantUsageStatus: AgentDescendantUsageStatus;
  totalUsage: AgentUsageMeasure | null;
  descendantSessionCount: number;
  precision: AgentUsagePrecision;
  partial: boolean;
  warnings: string[];
  sourceDimensions: AgentUsageSourceDimension[];
}

export interface AgentTaskUsageRow {
  appType: AgentUsageAppType;
  sessionId: string;
  rootSessionId: string;
  root: AgentSessionNodeView | null;
  selfUsage: AgentUsageMeasure | null;
  descendantUsage: AgentUsageMeasure | null;
  descendantUsageStatus: AgentDescendantUsageStatus;
  totalUsage: AgentUsageMeasure | null;
  descendantSessionCount: number;
  precision: AgentUsagePrecision;
  partial: boolean;
  warnings: string[];
  sourceDimensions: AgentUsageSourceDimension[];
}

export interface AgentTaskUsagePage {
  items: AgentTaskUsageRow[];
  total: number;
  limit: number;
  offset: number;
  hasMore: boolean;
  /** Codex proxy requests without verifiable native session attribution. */
  unattributedUsage: AgentUsageMeasure | null;
  /** Publication state while Codex canonical usage is rebuilt in the shadow generation. */
  dataStatus?: "ready" | "rebuilding_with_snapshot" | "rebuilding";
}

/** Backend default used whenever callers omit pagination values. */
export const AGENT_TASK_USAGE_DEFAULT_LIMIT = 50;
