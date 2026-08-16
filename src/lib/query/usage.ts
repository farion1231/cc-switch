import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { usageApi } from "@/lib/api/usage";
import { resolveUsageRange } from "@/lib/usageRange";
import type {
  LogFilters,
  UsageRangeSelection,
  UsageScopeFilters,
  AgentUsageAppType,
  AgentUsageRange,
  AgentSessionUsageSummary,
  AgentTaskUsageFilter,
  AgentTaskUsageFilterOptions,
  AgentTaskUsageFilterOptionsRequest,
  AgentTaskUsagePage,
  AgentUsageCapability,
} from "@/types/usage";
import { AGENT_TASK_USAGE_DEFAULT_LIMIT } from "@/types/usage";

const DEFAULT_REFETCH_INTERVAL_MS = 30000;

type UsageQueryOptions = {
  refetchInterval?: number | false;
  refetchIntervalInBackground?: boolean;
};

type RequestLogsQueryArgs = {
  filters: LogFilters;
  range: UsageRangeSelection;
  page?: number;
  pageSize?: number;
  options?: UsageQueryOptions;
};

type RequestLogsKey = {
  preset: UsageRangeSelection["preset"];
  customStartDate?: number;
  customEndDate?: number;
  liveEndTime?: boolean;
  appType?: string;
  providerName?: string;
  model?: string;
  statusCode?: number;
};

export interface NormalizedAgentUsageRange {
  startAt: number | null;
  endAt: number | null;
}

/**
 * Stable query identity for a UI range selection. The resolved end time is
 * deliberately not part of this shape: moving ranges are resolved by the
 * query function for every fetch, including polling/refetches.
 */
export interface NormalizedUsageRangeSelection {
  preset: UsageRangeSelection["preset"];
  customStartDate: number | null;
  customEndDate: number | null;
  liveEndTime: boolean;
}

/** Query-only filter envelope that keeps the public backend wire shape intact. */
export interface AgentTaskUsageQueryFilter
  extends Omit<AgentTaskUsageFilter, "range"> {
  range?: AgentUsageRange | null;
  rangeSelection?: UsageRangeSelection | null;
}

/** Query-only filter-options envelope; `rangeSelection` is never sent to Tauri. */
export interface AgentTaskUsageFilterOptionsQueryRequest
  extends Omit<AgentTaskUsageFilterOptionsRequest, "range"> {
  range?: AgentUsageRange | null;
  rangeSelection?: UsageRangeSelection | null;
}

/** Treat omitted/empty ranges as the same cache identity. */
export function normalizeAgentUsageRange(
  range?: AgentUsageRange | null,
): NormalizedAgentUsageRange {
  return {
    startAt: range?.startAt ?? null,
    endAt: range?.endAt ?? null,
  };
}

function denormalizeAgentUsageRange(
  range: NormalizedAgentUsageRange,
): AgentUsageRange | undefined {
  if (range.startAt === null && range.endAt === null) return undefined;
  return {
    ...(range.startAt === null ? {} : { startAt: range.startAt }),
    ...(range.endAt === null ? {} : { endAt: range.endAt }),
  };
}

export function normalizeUsageRangeSelection(
  selection?: UsageRangeSelection | null,
): NormalizedUsageRangeSelection | null {
  if (!selection) return null;
  return {
    preset: selection.preset,
    customStartDate: selection.customStartDate ?? null,
    customEndDate: selection.customEndDate ?? null,
    liveEndTime: selection.liveEndTime ?? false,
  };
}

function denormalizeUsageRangeSelection(
  selection: NormalizedUsageRangeSelection,
): UsageRangeSelection {
  return {
    preset: selection.preset,
    ...(selection.customStartDate === null
      ? {}
      : { customStartDate: selection.customStartDate }),
    ...(selection.customEndDate === null
      ? {}
      : { customEndDate: selection.customEndDate }),
    ...(selection.liveEndTime ? { liveEndTime: true } : {}),
  };
}

/** Resolve a query-only range at fetch time, never while constructing a key. */
function resolveAgentUsageQueryRange(
  selection: NormalizedUsageRangeSelection | null,
  range: NormalizedAgentUsageRange,
): AgentUsageRange | undefined {
  if (selection) {
    const { startDate, endDate } = resolveUsageRange(
      denormalizeUsageRangeSelection(selection),
    );
    return { startAt: startDate, endAt: endDate };
  }
  return denormalizeAgentUsageRange(range);
}

export interface NormalizedAgentTaskUsageFilter {
  appType: AgentUsageAppType | null;
  title: string | null;
  project: string | null;
  projectDir: string | null;
  titleExact: string | null;
  projectDirExact: string | null;
  range: NormalizedAgentUsageRange;
  rangeSelection: NormalizedUsageRangeSelection | null;
  limit: number;
  offset: number;
}

export function normalizeAgentTaskUsageFilter(
  filter: AgentTaskUsageQueryFilter = {},
): NormalizedAgentTaskUsageFilter {
  const rangeSelection = normalizeUsageRangeSelection(filter.rangeSelection);
  return {
    appType: filter.appType ?? null,
    title: filter.title ?? null,
    project: filter.project ?? null,
    projectDir: filter.projectDir ?? null,
    titleExact: filter.titleExact ?? null,
    projectDirExact: filter.projectDirExact ?? null,
    // A selection is authoritative. Ignoring any concurrently supplied
    // resolved range keeps moving timestamps out of the cache identity.
    range: normalizeAgentUsageRange(rangeSelection ? undefined : filter.range),
    rangeSelection,
    limit: filter.limit ?? AGENT_TASK_USAGE_DEFAULT_LIMIT,
    offset: filter.offset ?? 0,
  };
}

// Query keys
export const usageKeys = {
  all: ["usage"] as const,
  agent: ["usage", "agent-session"] as const,
  agentSession: (
    appType: AgentUsageAppType,
    sessionId: string,
    range: NormalizedAgentUsageRange,
  ) => [...usageKeys.agent, "session", appType, sessionId, range] as const,
  agentTasks: (filter: NormalizedAgentTaskUsageFilter) =>
    [
      ...usageKeys.agent,
      "tasks",
      filter.appType,
      filter.title,
      filter.project,
      filter.projectDir,
      filter.titleExact,
      filter.projectDirExact,
      filter.range,
      filter.rangeSelection,
      filter.limit,
      filter.offset,
    ] as const,
  agentTaskFilterOptions: (
    appType: AgentUsageAppType | null,
    range: NormalizedAgentUsageRange,
    rangeSelection: NormalizedUsageRangeSelection | null = null,
  ) =>
    [
      ...usageKeys.agent,
      "task-filter-options",
      appType,
      range,
      rangeSelection,
    ] as const,
  agentCapabilities: () => [...usageKeys.agent, "capabilities"] as const,
  summary: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: UsageScopeFilters,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "summary",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.appType ?? null,
      filters?.providerName ?? null,
      filters?.model ?? null,
    ] as const,
  summaryByApp: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: Pick<UsageScopeFilters, "providerName" | "model">,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "summary-by-app",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.providerName ?? null,
      filters?.model ?? null,
    ] as const,
  trends: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: UsageScopeFilters,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "trends",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.appType ?? null,
      filters?.providerName ?? null,
      filters?.model ?? null,
    ] as const,
  providerStats: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: UsageScopeFilters,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "provider-stats",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.appType ?? null,
      filters?.providerName ?? null,
      filters?.model ?? null,
    ] as const,
  modelStats: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: UsageScopeFilters,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "model-stats",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.appType ?? null,
      filters?.providerName ?? null,
      filters?.model ?? null,
    ] as const,
  logs: (key: RequestLogsKey, page: number, pageSize: number) =>
    [
      ...usageKeys.all,
      "logs",
      key.preset,
      key.customStartDate ?? 0,
      key.customEndDate ?? 0,
      key.liveEndTime ?? false,
      key.appType ?? "",
      key.providerName ?? "",
      key.model ?? "",
      key.statusCode ?? -1,
      page,
      pageSize,
    ] as const,
  detail: (requestId: string) =>
    [...usageKeys.all, "detail", requestId] as const,
  pricing: () => [...usageKeys.all, "pricing"] as const,
  limits: (providerId: string, appType: string) =>
    [...usageKeys.all, "limits", providerId, appType] as const,
  script: (providerId: string, appType: string) =>
    [...usageKeys.all, providerId, appType] as const,
};

/** 把 UI 侧的 "all" 哨兵归一成 undefined（后端语义：不过滤）。 */
function normalizeScopeFilters(filters?: UsageScopeFilters): UsageScopeFilters {
  return {
    appType: filters?.appType === "all" ? undefined : filters?.appType,
    providerName: filters?.providerName,
    model: filters?.model,
  };
}

// Hooks
export function useUsageSummary(
  range: UsageRangeSelection,
  filters?: UsageScopeFilters,
  options?: UsageQueryOptions,
) {
  const effective = normalizeScopeFilters(filters);
  return useQuery({
    queryKey: usageKeys.summary(
      range.preset,
      range.customStartDate,
      range.customEndDate,
      effective,
      range.liveEndTime,
    ),
    queryFn: () => {
      const { startDate, endDate } = resolveUsageRange(range);
      return usageApi.getUsageSummary(
        startDate,
        endDate,
        effective.appType,
        effective.providerName,
        effective.model,
      );
    },
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

export function useUsageSummaryByApp(
  range: UsageRangeSelection,
  filters?: Pick<UsageScopeFilters, "providerName" | "model">,
  options?: UsageQueryOptions,
) {
  return useQuery({
    queryKey: usageKeys.summaryByApp(
      range.preset,
      range.customStartDate,
      range.customEndDate,
      filters,
      range.liveEndTime,
    ),
    queryFn: () => {
      const { startDate, endDate } = resolveUsageRange(range);
      return usageApi.getUsageSummaryByApp(
        startDate,
        endDate,
        filters?.providerName,
        filters?.model,
      );
    },
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

export function useUsageTrends(
  range: UsageRangeSelection,
  filters?: UsageScopeFilters,
  options?: UsageQueryOptions,
) {
  const effective = normalizeScopeFilters(filters);
  return useQuery({
    queryKey: usageKeys.trends(
      range.preset,
      range.customStartDate,
      range.customEndDate,
      effective,
      range.liveEndTime,
    ),
    queryFn: () => {
      const { startDate, endDate } = resolveUsageRange(range);
      return usageApi.getUsageTrends(
        startDate,
        endDate,
        effective.appType,
        effective.providerName,
        effective.model,
      );
    },
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

export function useProviderStats(
  range: UsageRangeSelection,
  filters?: UsageScopeFilters,
  options?: UsageQueryOptions,
) {
  const effective = normalizeScopeFilters(filters);
  return useQuery({
    queryKey: usageKeys.providerStats(
      range.preset,
      range.customStartDate,
      range.customEndDate,
      effective,
      range.liveEndTime,
    ),
    queryFn: () => {
      const { startDate, endDate } = resolveUsageRange(range);
      return usageApi.getProviderStats(
        startDate,
        endDate,
        effective.appType,
        effective.providerName,
        effective.model,
      );
    },
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

export function useModelStats(
  range: UsageRangeSelection,
  filters?: UsageScopeFilters,
  options?: UsageQueryOptions,
) {
  const effective = normalizeScopeFilters(filters);
  return useQuery({
    queryKey: usageKeys.modelStats(
      range.preset,
      range.customStartDate,
      range.customEndDate,
      effective,
      range.liveEndTime,
    ),
    queryFn: () => {
      const { startDate, endDate } = resolveUsageRange(range);
      return usageApi.getModelStats(
        startDate,
        endDate,
        effective.appType,
        effective.providerName,
        effective.model,
      );
    },
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

export function useRequestLogs({
  filters,
  range,
  page = 0,
  pageSize = 20,
  options,
}: RequestLogsQueryArgs) {
  const key: RequestLogsKey = {
    preset: range.preset,
    customStartDate: range.customStartDate,
    customEndDate: range.customEndDate,
    liveEndTime: range.liveEndTime,
    appType: filters.appType,
    providerName: filters.providerName,
    model: filters.model,
    statusCode: filters.statusCode,
  };

  return useQuery({
    queryKey: usageKeys.logs(key, page, pageSize),
    queryFn: () => {
      const effectiveFilters = { ...filters, ...resolveUsageRange(range) };
      return usageApi.getRequestLogs(effectiveFilters, page, pageSize);
    },
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS, // 每30秒自动刷新
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

export function useRequestDetail(requestId: string) {
  return useQuery({
    queryKey: usageKeys.detail(requestId),
    queryFn: () => usageApi.getRequestDetail(requestId),
    enabled: !!requestId,
  });
}

export function useModelPricing() {
  return useQuery({
    queryKey: usageKeys.pricing(),
    queryFn: usageApi.getModelPricing,
  });
}

export function useProviderLimits(providerId: string, appType: string) {
  return useQuery({
    queryKey: usageKeys.limits(providerId, appType),
    queryFn: () => usageApi.checkProviderLimits(providerId, appType),
    enabled: !!providerId && !!appType,
  });
}

export function useUpdateModelPricing() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (params: {
      modelId: string;
      displayName: string;
      inputCost: string;
      outputCost: string;
      cacheReadCost: string;
      cacheCreationCost: string;
    }) =>
      usageApi.updateModelPricing(
        params.modelId,
        params.displayName,
        params.inputCost,
        params.outputCost,
        params.cacheReadCost,
        params.cacheCreationCost,
      ),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: usageKeys.all });
    },
  });
}

export function useDeleteModelPricing() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (modelId: string) => usageApi.deleteModelPricing(modelId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: usageKeys.all });
    },
  });
}

export interface AgentUsageQueryOptions {
  enabled?: boolean;
  staleTime?: number;
  refetchInterval?: number | false;
  refetchIntervalInBackground?: boolean;
}

/**
 * Query one selected session/root.  There is intentionally no
 * `placeholderData`/`keepPreviousData`: switching app/session/range must show
 * the new query's loading state rather than the previous session's usage.
 */
export function useAgentSessionUsage(
  appType: AgentUsageAppType,
  sessionId: string,
  range?: AgentUsageRange | null,
  options?: AgentUsageQueryOptions,
) {
  const normalizedRange = normalizeAgentUsageRange(range);
  return useQuery<AgentSessionUsageSummary>({
    queryKey: usageKeys.agentSession(appType, sessionId, normalizedRange),
    queryFn: () =>
      usageApi.getAgentSessionUsage({
        appType,
        sessionId,
        range: denormalizeAgentUsageRange(normalizedRange),
      }),
    enabled: (options?.enabled ?? true) && Boolean(appType && sessionId),
    staleTime: options?.staleTime,
    refetchInterval: options?.refetchInterval,
    refetchIntervalInBackground: options?.refetchIntervalInBackground,
  });
}

/** Query root/standalone task rows with every filter represented in its key. */
export function useAgentTaskUsage(
  filter: AgentTaskUsageQueryFilter = {},
  options?: AgentUsageQueryOptions,
) {
  const normalizedFilter = normalizeAgentTaskUsageFilter(filter);
  return useQuery<AgentTaskUsagePage>({
    queryKey: usageKeys.agentTasks(normalizedFilter),
    queryFn: () => {
      // Resolve the selection at fetch time so moving ranges advance on every
      // poll/refetch while the key remains bounded by stable fields.
      const range = resolveAgentUsageQueryRange(
        normalizedFilter.rangeSelection,
        normalizedFilter.range,
      );
      return usageApi.listAgentTaskUsage({
        appType: normalizedFilter.appType ?? undefined,
        title: normalizedFilter.title ?? undefined,
        project: normalizedFilter.project ?? undefined,
        projectDir: normalizedFilter.projectDir ?? undefined,
        titleExact: normalizedFilter.titleExact ?? undefined,
        projectDirExact: normalizedFilter.projectDirExact ?? undefined,
        range,
        limit: normalizedFilter.limit,
        offset: normalizedFilter.offset,
      });
    },
    enabled: options?.enabled ?? true,
    staleTime: options?.staleTime,
    refetchInterval: options?.refetchInterval,
    refetchIntervalInBackground: options?.refetchIntervalInBackground,
  });
}

/** Query the complete native title/project candidate list for a scope. */
export function useAgentTaskUsageFilterOptions(
  request: AgentTaskUsageFilterOptionsQueryRequest = {},
  options?: AgentUsageQueryOptions,
) {
  const rangeSelection = normalizeUsageRangeSelection(request.rangeSelection);
  const normalizedRange = normalizeAgentUsageRange(
    rangeSelection ? undefined : request.range,
  );
  const appType = request.appType ?? null;
  return useQuery<AgentTaskUsageFilterOptions>({
    queryKey: usageKeys.agentTaskFilterOptions(
      appType,
      normalizedRange,
      rangeSelection,
    ),
    queryFn: () => {
      // Keep filter-options requests on the same per-fetch range semantics as
      // task rows; otherwise a newly active task can be absent from choices.
      const range = resolveAgentUsageQueryRange(
        rangeSelection,
        normalizedRange,
      );
      return usageApi.getAgentTaskUsageFilterOptions({
        appType: request.appType,
        range,
      });
    },
    enabled: options?.enabled ?? true,
    staleTime: options?.staleTime,
    refetchInterval: options?.refetchInterval,
    refetchIntervalInBackground: options?.refetchIntervalInBackground,
  });
}

/** Query the backend-authoritative capability registry (all eight app IDs). */
export function useAgentUsageCapabilities(options?: AgentUsageQueryOptions) {
  return useQuery<AgentUsageCapability[]>({
    queryKey: usageKeys.agentCapabilities(),
    queryFn: usageApi.getAgentUsageCapabilities,
    enabled: options?.enabled ?? true,
    staleTime: options?.staleTime,
    refetchInterval: options?.refetchInterval,
    refetchIntervalInBackground: options?.refetchIntervalInBackground,
  });
}

// Explicit Query-suffixed aliases make the data layer easy to discover while
// retaining the concise naming convention used by the existing usage hooks.
export const useAgentSessionUsageQuery = useAgentSessionUsage;
export const useAgentTaskUsageQuery = useAgentTaskUsage;
export const useAgentTaskUsageFilterOptionsQuery =
  useAgentTaskUsageFilterOptions;
export const useAgentUsageCapabilitiesQuery = useAgentUsageCapabilities;
