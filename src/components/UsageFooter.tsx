import React from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { type AppId } from "@/lib/api";
import { useUsageQuery } from "@/lib/query/queries";
import { UsageData, Provider } from "@/types";
import {
  tierLabel,
  tierShortLabel,
} from "@/components/SubscriptionQuotaFooter";
import type { QuotaTier } from "@/types/subscription";
import { isAdditiveAppId } from "@/config/appConfig";
import { QuotaBars, QuotaLines } from "@/components/quota/QuotaLines";
import {
  balanceLine,
  expiredLine,
  failedLines,
  tierLine,
  type QuotaLine,
} from "@/components/quota/quotaRules";

interface UsageFooterProps {
  provider: Provider;
  providerId: string;
  appId: AppId;
  usageEnabled: boolean; // 是否启用了用量查询
  isCurrent: boolean; // 是否为当前激活的供应商
  isInConfig?: boolean; // OpenCode: 是否已添加到配置
  inline?: boolean; // 是否内联显示（在按钮左侧）
}

/** UsageData → QuotaTier 转换（Token Plan 使用） */
function toQuotaTier(data: UsageData): QuotaTier {
  const extra = data.extra;
  if (extra && extra.startsWith("{")) {
    try {
      const parsed = JSON.parse(extra);
      return {
        name: data.planName || "",
        utilization: data.used || 0,
        resetsAt: parsed.resetsAt || null,
        usedValueUsd: parsed.usedValueUsd ?? null,
        maxValueUsd: parsed.maxValueUsd ?? null,
        planLabel: parsed.planLabel ?? null,
      };
    } catch {
      // fall through to plain string
    }
  }
  return {
    name: data.planName || "",
    utilization: data.used || 0,
    resetsAt: extra || null,
  };
}

/** 脚本用量的一个套餐 → 一行（名字和明细进悬停说明） */
function planLine(
  t: TFunction,
  data: UsageData,
  index: number,
): QuotaLine | null {
  const key = `${index}-${data.planName ?? ""}`;
  const amount = (value: number) =>
    `${value === -1 ? "∞" : value.toFixed(2)}${data.unit ? ` ${data.unit}` : ""}`;
  const detail = [
    data.planName,
    data.total !== undefined ? `${t("usage.total")} ${amount(data.total)}` : "",
    data.used !== undefined ? `${t("usage.used")} ${amount(data.used)}` : "",
    data.extra,
  ]
    .filter(Boolean)
    .join(" · ");

  if (data.isValid === false) {
    return expiredLine(t, data.invalidMessage || detail || undefined, key);
  }
  if (data.remaining !== undefined) {
    return balanceLine(t, {
      key,
      remaining: data.remaining,
      total: data.total === -1 ? null : data.total,
      unit: data.unit,
      detail: detail || undefined,
    });
  }
  if (data.used !== undefined) {
    return {
      key,
      left: Infinity,
      tone: "normal",
      text: `${t("usage.used")} ${amount(data.used)}`,
      detail: detail || undefined,
    };
  }
  return null;
}

const UsageFooter: React.FC<UsageFooterProps> = ({
  provider,
  providerId,
  appId,
  usageEnabled,
  isCurrent,
  isInConfig = false,
  inline = false,
}) => {
  const { t } = useTranslation();
  const isTokenPlan =
    provider.meta?.usage_script?.templateType === "token_plan";

  // 统一的用量查询（自动查询仅对当前激活的供应商启用）
  // 累加模式：使用 isInConfig 代替 isCurrent
  const shouldAutoQuery = isAdditiveAppId(appId) ? isInConfig : isCurrent;
  const autoQueryInterval = shouldAutoQuery
    ? provider.meta?.usage_script?.autoQueryInterval || 0
    : 0;

  const {
    data: usage,
    isFetching: loading,
    isError,
    lastQueriedAt,
    refetch,
  } = useUsageQuery(providerId, appId, {
    enabled: usageEnabled,
    autoQueryInterval,
  });
  const refresh = () => void refetch();

  // 只在启用用量查询且有数据时显示。后端把瞬时传输失败转成了 reject：有缓存
  // 成功值时 react-query 保留 data 照常展示；首次查询就失败则 data 为空——
  // 此时（isError）仍要渲染失败态给出重试入口，否则 footer 整体消失、无从重查。
  if (!usageEnabled || (!usage && !isError)) return null;

  // 错误状态（业务失败，或无缓存成功值的 reject）
  if (!usage || !usage.success) {
    const reason = usage?.error || t("usage.queryFailed");
    if (inline) {
      return (
        <QuotaLines
          lines={failedLines(t, reason)}
          queriedAt={lastQueriedAt}
          loading={loading}
          onRefresh={refresh}
        />
      );
    }
    return (
      <QuotaBars
        className="mt-3"
        title={t("usage.planUsage")}
        rows={[]}
        loading={loading}
        onRefresh={refresh}
        footer={
          <p className="text-caption">
            <span className="font-medium text-danger-text">
              {t("quota.failed")}
            </span>{" "}
            <span className="text-fg-3">{reason}</span>
          </p>
        }
      />
    );
  }

  const usageDataList = usage.data || [];
  if (usageDataList.length === 0) return null;

  // Token Plan：按档写剩余百分比
  if (isTokenPlan) {
    const tiers = usageDataList.map((d) => toQuotaTier(d));
    const planLabel = tiers[0]?.planLabel;
    const rows = tiers.map((tier, index) => {
      const label = tierLabel(t, tier.name);
      const line = tierLine(t, tier, label, tierShortLabel(t, tier.name));
      return {
        label,
        line: {
          ...line,
          key: `${index}-${tier.name}`,
          detail: [planLabel, line.detail].filter(Boolean).join(" · "),
        },
      };
    });
    return inline ? (
      <QuotaLines
        lines={rows.map((row) => row.line)}
        queriedAt={lastQueriedAt}
        loading={loading}
        onRefresh={refresh}
      />
    ) : (
      <QuotaBars
        className="mt-3"
        title={planLabel || t("usage.planUsage")}
        rows={rows}
        queriedAt={lastQueriedAt}
        loading={loading}
        onRefresh={refresh}
      />
    );
  }

  const lines = usageDataList
    .map((data, index) => planLine(t, data, index))
    .filter((line): line is QuotaLine => line !== null);
  if (lines.length === 0) return null;

  if (inline) {
    return (
      <QuotaLines
        lines={lines}
        queriedAt={lastQueriedAt}
        loading={loading}
        onRefresh={refresh}
      />
    );
  }

  return (
    <QuotaBars
      className="mt-3"
      title={t("usage.planUsage")}
      rows={usageDataList.flatMap((data, index) => {
        const line = planLine(t, data, index);
        return line
          ? [
              {
                label: data.planName || "—",
                line: { ...line, value: line.text },
                note: data.extra,
              },
            ]
          : [];
      })}
      queriedAt={lastQueriedAt}
      loading={loading}
      onRefresh={refresh}
    />
  );
};

export default UsageFooter;
