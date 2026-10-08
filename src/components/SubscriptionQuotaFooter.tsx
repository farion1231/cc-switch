import React from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";
import { useSubscriptionQuota } from "@/lib/query/subscription";
import type { QuotaTier, SubscriptionQuota } from "@/types/subscription";
import { QuotaBars, QuotaLines } from "@/components/quota/QuotaLines";
import {
  creditsBreakdownItem,
  creditsLine,
  failedLines,
  resetCreditsLine,
  tierLine,
  type QuotaLine,
} from "@/components/quota/quotaRules";

export { countdownStr } from "@/components/quota/quotaRules";

interface SubscriptionQuotaFooterProps {
  appId: AppId;
  inline?: boolean;
  isCurrent?: boolean;
  autoQueryInterval?: number;
}

interface SubscriptionQuotaViewProps {
  quota: SubscriptionQuota | undefined;
  loading: boolean;
  /** 原样传 refetch：额度列靠它返回的结果判断点击重查的成败 */
  refetch: () => unknown;
  /** 用于 `subscription.expiredHint` 的 {tool} 插值；解耦了 hook 的 appId */
  appIdForExpiredHint: string;
  inline?: boolean;
}

/** 已知 tier 名称的显示映射（官方订阅 + Token Plan 共用） */
export const TIER_I18N_KEYS: Record<string, string> = {
  five_hour: "subscription.fiveHour",
  seven_day: "subscription.sevenDay",
  seven_day_fable: "subscription.sevenDayFable",
  seven_day_opus: "subscription.sevenDayOpus",
  seven_day_sonnet: "subscription.sevenDaySonnet",
  // Codex 免费方案的次要窗口是 30 天（付费方案为 7 天）
  "30_day": "subscription.thirtyDay",
  // Gemini 模型分类
  gemini_pro: "subscription.geminiPro",
  gemini_flash: "subscription.geminiFlash",
  gemini_flash_lite: "subscription.geminiFlashLite",
  // Token Plan（five_hour 已在上方官方映射中）
  weekly_limit: "subscription.sevenDay",
  // 火山方舟 Agent Plan / Coding Plan 的月窗口
  monthly: "subscription.monthly",
  // Grok credit 额度的兜底窗口（重置距离可识别时归入 weekly_limit/monthly）
  credits: "subscription.credits",
  // GitHub Copilot
  premium: "subscription.copilotPremium",
};

/** 卡片上不显示的档（展开时仍列出） */
const HIDDEN_INLINE_TIERS = new Set(["seven_day_sonnet"]);

export function tierLabel(t: TFunction, name: string): string {
  return TIER_I18N_KEYS[name] ? t(TIER_I18N_KEYS[name]) : name;
}

/** 卡片合并行里的短档名（`subscription.short.*`，英日用缩写） */
export function tierShortLabel(t: TFunction, name: string): string {
  const key = TIER_I18N_KEYS[name];
  return key ? t(key.replace("subscription.", "subscription.short.")) : name;
}

/** 已知档位 → 额度行 */
export function tierLines(
  t: TFunction,
  tiers: QuotaTier[],
  { inline = false }: { inline?: boolean } = {},
): { label: string; line: QuotaLine }[] {
  return tiers
    .filter((tier) => tier.name in TIER_I18N_KEYS)
    .filter((tier) => !inline || !HIDDEN_INLINE_TIERS.has(tier.name))
    .map((tier) => {
      const label = tierLabel(t, tier.name);
      return {
        label,
        line: tierLine(t, tier, label, tierShortLabel(t, tier.name)),
      };
    });
}

/**
 * 一份查询成功的订阅额度要画的行：各档，再加存下的重置次数和 Credits 余额（只有 ChatGPT 订阅有）。
 * Credits 要等额度用完才扣，卡片上平时不占位置（点开重置次数能看到），有一档用完才露出来，并排在重置次数前面：
 * 卡片合并行放不下时按剩余挑段，两者都没有比例、谁在前留谁（见 pickLines）
 */
export function quotaRows(
  t: TFunction,
  quota: SubscriptionQuota,
  locale: string,
  { inline = false }: { inline?: boolean } = {},
): { label: string; line: QuotaLine }[] {
  const rows = tierLines(t, quota.tiers || [], { inline });
  // 一档都没有时额度整块不显示，重置次数、余额也不单独出来
  if (rows.length === 0) return rows;
  const balance = creditsLine(t, quota.creditsBalance, { locale });
  // 卡片上点开重置次数时附带余额（展开时余额自己有一行，不重复）
  const resets = resetCreditsLine(t, quota.resetCredits, {
    locale,
    footer: inline && balance ? [creditsBreakdownItem(t, balance)] : undefined,
  });
  const credits =
    inline && !rows.some((row) => row.line.left <= 0) ? null : balance;
  const resetsRow = resets && {
    label: t("quota.resetCredits.label"),
    line: resets,
  };
  const creditsRow = credits && {
    label: t("quota.credits.label"),
    line: credits,
  };
  const extras = inline ? [creditsRow, resetsRow] : [resetsRow, creditsRow];
  return [
    ...rows,
    ...extras.filter((row): row is NonNullable<typeof row> => Boolean(row)),
  ];
}

/** 额度没查到的原因：登录过期 / 令牌待刷新写固定文案，其余写后端给的错误 */
export function quotaFailureReason(
  t: TFunction,
  quota: SubscriptionQuota,
): string {
  if (quota.credentialStatus === "expired") {
    return t("quota.reason.loginExpired");
  }
  if (quota.credentialStatus === "refresh_pending") {
    return t("quota.reason.tokenRefreshPending");
  }
  return quota.error || t("subscription.queryFailed");
}

/**
 * 纯展示组件：渲染 SubscriptionQuota 的状态（not_found / parse_error 不显示；
 * 登录过期 / 令牌待刷新 / 查询失败写「额度没查到」+ 原因；成功按档写剩余），支持卡片（inline）和展开两种布局。
 *
 * 数据源由调用方 hook 注入，方便不同的额度后端复用同一套渲染逻辑：
 * - `SubscriptionQuotaFooter`（CLI 凭据路径，by appId）
 * - `CodexOauthQuotaFooter`（cc-switch 自管 OAuth 路径，by ChatGPT account）
 */
export const SubscriptionQuotaView: React.FC<SubscriptionQuotaViewProps> = ({
  quota,
  loading,
  refetch,
  appIdForExpiredHint,
  inline = false,
}) => {
  const { t, i18n } = useTranslation();

  // 无凭据 / 凭据解析错误 → 不显示（静默）
  if (!quota || quota.credentialStatus === "not_found") return null;
  if (quota.credentialStatus === "parse_error") return null;

  if (!quota.success) {
    // 登录过期和令牌待刷新都是运行一次 CLI 就能解决，展开时给同一条提示
    const expired =
      quota.credentialStatus === "expired" ||
      quota.credentialStatus === "refresh_pending";
    const reason = quotaFailureReason(t, quota);
    const lines = failedLines(t, reason);
    if (inline) {
      return (
        <QuotaLines
          lines={lines}
          queriedAt={quota.queriedAt}
          loading={loading}
          onRefresh={refetch}
        />
      );
    }
    return (
      <QuotaBars
        className="mt-3"
        title={t("subscription.title")}
        rows={[]}
        loading={loading}
        onRefresh={refetch}
        footer={
          <p className="text-caption">
            <span className="font-medium text-danger-text">
              {t("quota.failed")}
            </span>{" "}
            <span className="text-fg-3">
              {expired
                ? t("subscription.expiredHint", { tool: appIdForExpiredHint })
                : reason}
            </span>
          </p>
        }
      />
    );
  }

  const rows = quotaRows(t, quota, i18n.language, { inline });
  if (rows.length === 0) return null;

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

  const extra = quota.extraUsage;
  return (
    <QuotaBars
      className="mt-3"
      title={t("subscription.title", { defaultValue: "Subscription Quota" })}
      rows={rows}
      queriedAt={quota.queriedAt}
      loading={loading}
      onRefresh={refetch}
      footer={
        extra?.isEnabled ? (
          <div className="mt-2 border-t border-border pt-2 text-caption text-fg-2">
            <span className="font-medium">
              {t("subscription.extraUsage")}:{" "}
            </span>
            <span className="tabular-nums">
              {extra.currency === "USD" ? "$" : ""}
              {(extra.usedCredits ?? 0).toFixed(2)}
              {extra.monthlyLimit != null && (
                <>
                  {" "}
                  / {extra.currency === "USD" ? "$" : ""}
                  {extra.monthlyLimit.toFixed(2)}
                </>
              )}
            </span>
          </div>
        ) : null
      }
    />
  );
};

/**
 * CLI 凭据路径下的薄 wrapper：通过 useSubscriptionQuota(appId) 自取数据
 * 后转发到 SubscriptionQuotaView。
 */
const SubscriptionQuotaFooter: React.FC<SubscriptionQuotaFooterProps> = ({
  appId,
  inline = false,
  isCurrent = false,
  autoQueryInterval = 5,
}) => {
  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useSubscriptionQuota(
    appId,
    isCurrent,
    isCurrent && autoQueryInterval > 0,
    autoQueryInterval,
  );

  if (!isCurrent) return null;

  return (
    <SubscriptionQuotaView
      quota={quota}
      loading={loading}
      refetch={refetch}
      // expiredHint 里的 {tool} 是 CLI 命令名：Grok 的命令是 `grok` 而非 appId
      appIdForExpiredHint={appId === "grokbuild" ? "grok" : appId}
      inline={inline}
    />
  );
};

export default SubscriptionQuotaFooter;
