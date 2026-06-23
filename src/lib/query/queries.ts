import { useRef } from "react";
import {
  useQuery,
  type UseQueryResult,
  keepPreviousData,
} from "@tanstack/react-query";
import {
  providersApi,
  settingsApi,
  usageApi,
  sessionsApi,
  type AppId,
} from "@/lib/api";
import type {
  Provider,
  Settings,
  UsageResult,
  SessionMeta,
  SessionMessage,
} from "@/types";
import { usageKeys } from "@/lib/query/usage";

const sortProviders = (
  providers: Record<string, Provider>,
): Record<string, Provider> => {
  const sortedEntries = Object.values(providers)
    .sort((a, b) => {
      const indexA = a.sortIndex ?? Number.MAX_SAFE_INTEGER;
      const indexB = b.sortIndex ?? Number.MAX_SAFE_INTEGER;
      if (indexA !== indexB) {
        return indexA - indexB;
      }

      const timeA = a.createdAt ?? 0;
      const timeB = b.createdAt ?? 0;
      if (timeA === timeB) {
        return a.name.localeCompare(b.name, "zh-CN");
      }
      return timeA - timeB;
    })
    .map((provider) => [provider.id, provider] as const);

  return Object.fromEntries(sortedEntries);
};

export interface ProvidersQueryData {
  providers: Record<string, Provider>;
  currentProviderId: string;
}

export interface UseProvidersQueryOptions {
  isProxyRunning?: boolean; // 代理服务是否运行中
}

export const useProvidersQuery = (
  appId: AppId,
  options?: UseProvidersQueryOptions,
): UseQueryResult<ProvidersQueryData> => {
  const { isProxyRunning = false } = options || {};

  return useQuery({
    queryKey: ["providers", appId],
    placeholderData: keepPreviousData,
    // 当代理服务运行时，每 10 秒刷新一次供应商列表
    // 这样可以自动反映后端熔断器自动禁用代理目标的变更
    refetchInterval: isProxyRunning ? 10000 : false,
    queryFn: async () => {
      let providers: Record<string, Provider> = {};
      let currentProviderId = "";

      try {
        providers = await providersApi.getAll(appId);
      } catch (error) {
        console.error("获取供应商列表失败:", error);
      }

      try {
        currentProviderId = await providersApi.getCurrent(appId);
      } catch (error) {
        console.error("获取当前供应商失败:", error);
      }

      return {
        providers: sortProviders(providers),
        currentProviderId,
      };
    },
  });
};

export const useSettingsQuery = (): UseQueryResult<Settings> => {
  return useQuery({
    queryKey: ["settings"],
    queryFn: async () => settingsApi.get(),
  });
};

export interface UseUsageQueryOptions {
  enabled?: boolean;
  autoQueryInterval?: number; // 自动查询间隔（分钟），0 表示禁用
}

/** 最近一次成功的用量结果快照（keep-last-good 用）。 */
export interface LastGoodUsage {
  data: UsageResult;
  at: number; // 该成功结果的获取时刻（ms）
}

/** 在最近一次成功后多久内，失败仍继续展示该成功值。 */
export const KEEP_LAST_GOOD_MS = 10 * 60 * 1000; // 10 分钟

/**
 * 判断一次用量查询失败是否属于"瞬时/网络类"（可被 keep-last-good 短暂掩盖）。
 *
 * 仅瞬时失败才允许继续展示上一次成功；**确定性失败**（鉴权失败、空 API Key、
 * 未知供应商、4xx、脚本/解析错误等）必须立即透出——用户改/删凭据后要马上看到，
 * 否则会一直显示过期额度直到窗口结束。
 *
 * 采用**白名单**：只认后端稳定的网络类错误前缀 + HTTP 5xx，失败安全——任何未识别
 * 的错误一律按"非瞬时"立即透出，绝不误掩盖确定性失败。需与后端错误文案保持同步：
 * - 原生 balance/coding_plan/subscription：reqwest send 失败/超时 → `"Network error: …"`；
 *   上游非 2xx → `"API error (HTTP <code>…)"`
 * - JS 脚本 usage_script：`request_failed` → "请求失败/Request failed"、
 *   `read_response_failed` → "读取响应失败/Failed to read response"（仅 zh/en 两种本地化）；
 *   上游非 2xx → `"HTTP <code> …"`
 *
 * HTTP 状态：**5xx**（服务端错误，通常瞬时）归为 transient；**4xx**（鉴权/客户端
 * 错误，如 401/403/404/429）保持确定性，立即透出。
 */
export function isTransientUsageError(result: UsageResult): boolean {
  if (result.success) return false;
  const e = result.error?.toLowerCase() ?? "";
  if (!e) return false;

  // 网络类（send 失败/超时/读取响应失败）
  if (
    e.includes("network error") || // 原生路径
    e.includes("request failed") || // JS 脚本 (en)
    e.includes("请求失败") || // JS 脚本 (zh)
    e.includes("failed to read response") || // JS 脚本 (en)
    e.includes("读取响应失败") // JS 脚本 (zh)
  ) {
    return true;
  }

  // HTTP 状态码：5xx 视为瞬时，4xx 视为确定性。错误文案里第一处 "HTTP <code>" 即为
  // 上游状态码（原生 "API error (HTTP 500…)"、JS 脚本 "HTTP 500 …"）。
  const httpMatch = e.match(/http\s+(\d{3})/);
  if (httpMatch) {
    const status = Number(httpMatch[1]);
    return status >= 500 && status <= 599;
  }

  return false;
}

/**
 * Keep-last-good 的纯决策函数（无 ref、无时钟，`now` 注入以便测试）。
 *
 * 用量端点面向跨境/第三方，单次网络抖动会让后端返回 `Ok(success:false)` 当作"新数据"
 * 覆盖掉上一次成功，使卡片瞬间翻红——这是"一会成功一会失败"观感的主因。
 * 策略：当失败是**瞬时/网络类**（见 [`isTransientUsageError`]）且最近一次成功在
 * `keepMs` 内时，不抹掉它，继续展示该成功值，并把 `lastQueriedAt` 指向该成功的时刻
 * （相对时间自然走到"10 分钟前"后过期翻红）；超出窗口、或从无成功记录时照常展示失败。
 *
 * **确定性失败**（鉴权/空 key/未知供应商/4xx 等）不仅立即透出，还会**清空 `lastGood`**：
 * 旧成功快照已不可信，否则随后一次网络抖动会把"配置/鉴权已失效"的旧额度重新复活。
 *
 * 注：会 reject 的传输层失败（Copilot/DB）react-query 本就保留上次 `data`，这里
 * 主要修的是 `Ok(success:false)` 这条覆盖路径。
 */
export function resolveDisplayUsage(
  raw: UsageResult | undefined,
  dataUpdatedAt: number,
  prevLastGood: LastGoodUsage | null,
  now: number,
  keepMs: number = KEEP_LAST_GOOD_MS,
): {
  data: UsageResult | undefined;
  lastQueriedAt: number | null;
  lastGood: LastGoodUsage | null;
} {
  let lastGood = prevLastGood;
  if (raw?.success) {
    // 成功：刷新快照
    lastGood = { data: raw, at: dataUpdatedAt || now };
  } else if (raw && !isTransientUsageError(raw)) {
    // 确定性失败（鉴权/空 key/未知供应商/4xx 等）：旧成功快照已不可信，丢弃它，
    // 避免后续一次网络抖动把"配置/鉴权已失效"的旧额度重新复活。
    lastGood = null;
  }

  let data = raw;
  let lastQueriedAt = dataUpdatedAt || null;
  if (
    raw &&
    !raw.success &&
    isTransientUsageError(raw) && // 仅瞬时/网络类失败才掩盖；确定性失败立即透出
    lastGood &&
    now - lastGood.at < keepMs
  ) {
    data = lastGood.data;
    lastQueriedAt = lastGood.at;
  }

  return { data, lastQueriedAt, lastGood };
}

export const useUsageQuery = (
  providerId: string,
  appId: AppId,
  options?: UseUsageQueryOptions,
) => {
  const { enabled = true, autoQueryInterval = 0 } = options || {};

  // 计算 staleTime：如果有自动刷新间隔，使用该间隔；否则默认 5 分钟
  // 这样可以避免切换 app 页面时重复触发查询
  const staleTime =
    autoQueryInterval > 0
      ? autoQueryInterval * 60 * 1000 // 与刷新间隔保持一致
      : 5 * 60 * 1000; // 默认 5 分钟

  const query = useQuery<UsageResult>({
    queryKey: usageKeys.script(providerId, appId),
    queryFn: async () => usageApi.query(providerId, appId),
    enabled: enabled && !!providerId,
    refetchInterval:
      autoQueryInterval > 0
        ? Math.max(autoQueryInterval, 1) * 60 * 1000 // 最小1分钟
        : false,
    refetchIntervalInBackground: true, // 后台也继续定时查询
    refetchOnWindowFocus: false,
    // 用量查询面向跨境/第三方端点，单次网络抖动或瞬时 5xx 不应直接判失败。
    // 重试一次以吸收瞬时故障（与 useSubscriptionQuota 的 retry:1 保持一致）。
    // 注意：原生 balance/coding_plan 路径把网络错误折叠成 Ok(success:false)，
    // 这类不会触发 react-query 重试；本项主要覆盖会 reject 的传输层失败（Copilot/DB 等）。
    retry: 1,
    retryDelay: 1500,
    staleTime, // 使用动态计算的缓存时间
    gcTime: 10 * 60 * 1000, // 缓存保留 10 分钟（组件卸载后）
  });

  // Keep-last-good：失败时在 10 分钟窗口内继续展示上一次成功值（见 resolveDisplayUsage）。
  // 每个 hook 实例各持一份 ref（按卡片维度）；ref 写入是幂等的（同份成功重复写无副作用）。
  const lastGoodRef = useRef<LastGoodUsage | null>(null);
  const { data, lastQueriedAt, lastGood } = resolveDisplayUsage(
    query.data,
    query.dataUpdatedAt,
    lastGoodRef.current,
    Date.now(),
  );
  lastGoodRef.current = lastGood;

  return {
    ...query,
    data,
    lastQueriedAt,
  };
};

export const useSessionsQuery = () => {
  return useQuery<SessionMeta[]>({
    queryKey: ["sessions"],
    queryFn: async () => sessionsApi.list(),
    staleTime: 30 * 1000,
  });
};

export const useSessionMessagesQuery = (
  providerId?: string,
  sourcePath?: string,
) => {
  return useQuery<SessionMessage[]>({
    queryKey: ["sessionMessages", providerId, sourcePath],
    queryFn: async () => sessionsApi.getMessages(providerId!, sourcePath!),
    enabled: Boolean(providerId && sourcePath),
    staleTime: 30 * 1000,
  });
};
