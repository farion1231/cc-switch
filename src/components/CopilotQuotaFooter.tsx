import React from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import type { ProviderMeta } from "@/types";
import {
  useCopilotQuota,
  type CopilotQuotaKind,
  type CopilotQuotaTier,
} from "@/lib/query/copilot";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { PROVIDER_TYPES } from "@/config/constants";
import { QuotaBars, QuotaLines } from "@/components/quota/QuotaLines";
import { useQuotaDisplay } from "@/components/quota/useQuotaDisplay";
import {
  failedLines,
  tierLine,
  type QuotaDisplay,
  type QuotaLine,
} from "@/components/quota/quotaRules";

const COPILOT_TIER_LABEL_KEYS: Record<CopilotQuotaKind, string> = {
  premium: "subscription.copilotPremium",
  completions: "subscription.copilotCompletions",
  chat: "subscription.copilotChat",
};

/**
 * Copilot 各项额度 → 额度行。只画百分比的话，Max 和 Free 月初都是「剩余 100%」，
 * 所以把「剩余 x / 共 y」写进悬停说明；plan 给了就排在前面。计划不含的项写「不含」。
 */
export function copilotQuotaRows(
  t: TFunction,
  tiers: CopilotQuotaTier[],
  {
    display,
    locale,
    plan,
  }: { display: QuotaDisplay; locale: string; plan?: string | null },
): { label: string; line: QuotaLine }[] {
  const fmt = (n: number) => n.toLocaleString(locale);
  return tiers.map((tier) => {
    const label = t(COPILOT_TIER_LABEL_KEYS[tier.name]);
    if (!tier.included) {
      // 额度条的位置写一句灰字，数值格空着
      const caption = t("subscription.copilotNotIncluded");
      return {
        label,
        line: {
          key: tier.name,
          text: caption,
          caption,
          value: "",
          tone: "normal",
          left: 100,
          label,
          detail: plan || undefined,
        },
      };
    }
    const count = t("subscription.copilotCount", {
      remaining: fmt(tier.remaining),
      total: fmt(tier.entitlement),
    });
    return {
      label,
      line: {
        ...tierLine(t, tier, label, { display }),
        detail: [plan, count].filter(Boolean).join(" · "),
      },
    };
  });
}

interface CopilotQuotaFooterProps {
  meta?: ProviderMeta;
  inline?: boolean;
  /** 是否为当前激活的供应商 */
  isCurrent?: boolean;
}

const CopilotQuotaFooter: React.FC<CopilotQuotaFooterProps> = ({
  meta,
  inline = false,
  isCurrent = false,
}) => {
  const { t, i18n } = useTranslation();
  const display = useQuotaDisplay();
  const accountId = resolveManagedAccountId(
    meta,
    PROVIDER_TYPES.GITHUB_COPILOT,
  );

  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useCopilotQuota(accountId, { enabled: true, autoQuery: isCurrent });

  if (!quota) return null;

  if (!quota.success) {
    return inline ? (
      <QuotaLines
        lines={failedLines(t, quota.error || t("subscription.queryFailed"))}
        queriedAt={quota.queriedAt}
        loading={loading}
        onRefresh={refetch}
      />
    ) : null;
  }

  // 卡片上位置紧，计划不含的项不占一行；授权中心的账号行照列
  const tiers = inline
    ? quota.tiers.filter((tier) => tier.included)
    : quota.tiers;
  if (tiers.length === 0) return null;
  const rows = copilotQuotaRows(t, tiers, {
    display,
    locale: i18n.language,
    plan: quota.plan,
  });

  if (inline) {
    return (
      <QuotaLines
        lines={rows.map((row) => row.line)}
        queriedAt={quota.queriedAt}
        loading={loading}
        onRefresh={refetch}
      />
    );
  }

  return (
    <QuotaBars
      className="mt-3"
      title={quota.plan || t("subscription.title")}
      rows={rows}
      queriedAt={quota.queriedAt}
      loading={loading}
      onRefresh={refetch}
    />
  );
};

export default CopilotQuotaFooter;
