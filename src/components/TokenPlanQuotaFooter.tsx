import React from "react";
import { RefreshCw, AlertCircle, Clock } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ProviderMeta } from "@/types";
import type { AppId } from "@/lib/api";
import { TEMPLATE_TYPES } from "@/config/constants";
import {
  TierBadge,
  TIER_I18N_KEYS,
} from "@/components/SubscriptionQuotaFooter";
import { useUsageQuery } from "@/lib/query/queries";

interface TokenPlanQuotaFooterProps {
  providerId: string;
  appId: AppId;
  meta?: ProviderMeta;
  inline?: boolean;
}

/** 格式化相对时间（与 UsageFooter 一致） */
function formatRelativeTime(
  timestamp: number,
  now: number,
  t: (key: string, options?: { count?: number }) => string,
): string {
  const diff = Math.floor((now - timestamp) / 1000);
  if (diff < 60) return t("usage.justNow");
  if (diff < 3600)
    return t("usage.minutesAgo", { count: Math.floor(diff / 60) });
  if (diff < 86400)
    return t("usage.hoursAgo", { count: Math.floor(diff / 3600) });
  return t("usage.daysAgo", { count: Math.floor(diff / 86400) });
}

/** 从 UsageData.extra 里解析结构化字段：可能是 JSON（含 resetsAt/usedCount/totalCount/countUnit），
 *  也可能只是 ISO 字符串（旧 OFFICIAL_SUBSCRIPTION 路径）。
 */
function parseExtraObject(extra: string | undefined | null): {
  resetsAt: string | null;
  usedCount: number | null;
  totalCount: number | null;
  countUnit: string | null;
  remainsTimeMs: number | null;
} {
  const empty = {
    resetsAt: null,
    usedCount: null,
    totalCount: null,
    countUnit: null,
    remainsTimeMs: null,
  };
  if (!extra) return empty;
  const trimmed = extra.trim();
  if (!trimmed) return empty;
  if (!trimmed.startsWith("{")) {
    // 纯 ISO 字符串（旧路径：直接当 resetsAt 用）
    return { ...empty, resetsAt: trimmed };
  }
  try {
    const obj = JSON.parse(trimmed);
    return {
      resetsAt: typeof obj?.resetsAt === "string" ? obj.resetsAt : null,
      usedCount: typeof obj?.usedCount === "number" ? obj.usedCount : null,
      totalCount: typeof obj?.totalCount === "number" ? obj.totalCount : null,
      countUnit: typeof obj?.countUnit === "string" ? obj.countUnit : null,
      remainsTimeMs:
        typeof obj?.remainsTimeMs === "number" ? obj.remainsTimeMs : null,
    };
  } catch {
    return empty;
  }
}

/**
 * Coding Plan (TOKEN_PLAN) 供应商的 5h/7d 配额条。
 *
 * 数据来源：与 provider 级 `useUsageQuery` 共享同一份缓存——Rust 端的
 * `query_special_template` 把 Coding Plan 的 5h/7d tier 列表构造成
 * `UsageData[]`，这里直接渲染。
 *
 * **不在 per-key query 路径上重复**：Coding Plan 配额是账号级而非 key 级——
 * 同一个账号的 N 把 key 共用一份 5h/7d，复制到每一行只会误导用户。
 * ProviderCard 的 `hasKeyPool` 分支不再吞掉 inline 槽，本组件始终可见；
 * KeyPoolList 在 TOKEN_PLAN 下也不渲染 usage 条。
 */
export const TokenPlanQuotaFooter: React.FC<TokenPlanQuotaFooterProps> = ({
  providerId,
  appId,
  meta,
  inline = false,
}) => {
  const { t } = useTranslation();

  // 复用 useUsageQuery：autoQueryInterval 由 provider.meta.usage_script 控制。
  // 数据由 ProviderCard 上层同样的 useUsageQuery 写进 React Query 缓存
  // （key 是 `usageKeys.script(providerId, appId)`），本组件只是消费者——不要
  // 再加 `enabled: isCurrent` 之类的门控，否则缓存里有数据也会被跳过，
  // inline 槽空空如也。ProviderCard 自己的 useUsageQuery 已经判过
  // usageEnabled / isOfficialSubscriptionUsage，这里共享同一条 query。
  const {
    data,
    isFetching,
    refetch,
    dataUpdatedAt,
  } = useUsageQuery(providerId, appId, {
    autoQueryInterval: meta?.usage_script?.autoQueryInterval ?? 1,
  });

  // 30 秒刷一次「相对时间」—— hooks 必须在 early return 之前调用，遵守
  // Rules of Hooks。dataUpdatedAt 变化时重建定时器。
  const [now, setNow] = React.useState(Date.now());
  React.useEffect(() => {
    if (!dataUpdatedAt) return;
    const id = setInterval(() => setNow(Date.now()), 30000);
    return () => clearInterval(id);
  }, [dataUpdatedAt]);

  if (!data) return null;

  if (!data.success) {
    if (inline) {
      return (
        <div className="inline-flex items-center gap-2 text-xs rounded-lg border border-border-default bg-card px-3 py-2 shadow-sm">
          <div className="flex items-center gap-1.5 text-red-500 dark:text-red-400">
            <AlertCircle size={12} />
            <span>{data.error || t("subscription.queryFailed")}</span>
          </div>
          <button
            onClick={() => refetch()}
            disabled={isFetching}
            className="p-1 rounded hover:bg-muted transition-colors disabled:opacity-50 flex-shrink-0"
            title={t("subscription.refresh")}
          >
            <RefreshCw size={12} className={isFetching ? "animate-spin" : ""} />
          </button>
        </div>
      );
    }
    return null;
  }

  const tiers = (data.data || []).filter((d) => {
    const name = (d.planName ?? "").trim();
    return name in TIER_I18N_KEYS;
  });
  if (tiers.length === 0) return null;

  if (inline) {
    return (
      <div className="flex flex-col items-end gap-1 text-xs whitespace-nowrap flex-shrink-0">
        <div className="flex items-center gap-2 justify-end">
          <span className="text-[10px] text-muted-foreground/70 flex items-center gap-1">
            <Clock size={10} />
            {dataUpdatedAt
              ? formatRelativeTime(dataUpdatedAt, now, t)
              : t("usage.never", { defaultValue: "从未更新" })}
          </span>
          <button
            onClick={() => refetch()}
            disabled={isFetching}
            className="p-1 rounded hover:bg-muted transition-colors disabled:opacity-50 flex-shrink-0 text-muted-foreground"
            title={t("subscription.refresh")}
          >
            <RefreshCw size={12} className={isFetching ? "animate-spin" : ""} />
          </button>
        </div>
        <div className="flex items-center gap-2">
          {tiers.map((tier) => {
            const parsed = parseExtraObject(tier.extra);
            return (
              <TierBadge
                key={(tier.planName ?? "").trim()}
                tier={{
                  name: (tier.planName ?? "").trim(),
                  utilization: tier.used ?? 0,
                  resetsAt: parsed.resetsAt,
                  // 透传 absolute count（MiniMax / Zhipu），让 TierBadge 走 hasAbsolute 分支
                  // 渲染「X / Y count」+ 真实百分比（可超 100%）。
                  usedCount: parsed.usedCount,
                  totalCount: parsed.totalCount,
                  countUnit: parsed.countUnit,
                  // 透传服务端精确剩余毫秒数,优先于 end_time - Date.now()。
                  remainsTimeMs: parsed.remainsTimeMs,
                }}
                t={t}
              />
            );
          })}
        </div>
      </div>
    );
  }

  return null;
};

/** 类型守卫：ProviderCard 用它判断是否走 TOKEN_PLAN 渲染分支 */
export function isTokenPlanProvider(meta?: ProviderMeta): boolean {
  return meta?.usage_script?.templateType === TEMPLATE_TYPES.TOKEN_PLAN;
}