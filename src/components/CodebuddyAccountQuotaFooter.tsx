import React from "react";
import { useTranslation } from "react-i18next";
import { useCodebuddyAccountQuota } from "@/lib/query/subscription";
import { codebuddyQuotaState } from "@/components/settings/auth/AccountQuota";
import { QuotaLines } from "@/components/quota/QuotaLines";
import { failedLines } from "@/components/quota/quotaRules";
import { useQuotaDisplay } from "@/components/quota/useQuotaDisplay";
import { extractErrorMessage } from "@/utils/errorUtils";

interface CodebuddyAccountQuotaFooterProps {
  accountId: string;
}

/**
 * CodeBuddy 官方登录卡右侧的套餐积分：查卡片绑定的那个号（不用先切号），
 * 和授权中心那一行共用同一份查询；卡片上写成一行「套餐名 剩余积分」。
 */
const CodebuddyAccountQuotaFooter: React.FC<
  CodebuddyAccountQuotaFooterProps
> = ({ accountId }) => {
  const { t, i18n } = useTranslation();
  const display = useQuotaDisplay();
  const lang = i18n.language?.toLowerCase().startsWith("zh") ? "zh" : "en";
  const {
    data: quota,
    isFetching: loading,
    isError,
    error,
    refresh,
  } = useCodebuddyAccountQuota(accountId, lang);
  const state =
    !quota && isError
      ? {
          kind: "failed" as const,
          reason: extractErrorMessage(error) || t("subscription.queryFailed"),
        }
      : codebuddyQuotaState(t, quota, loading, i18n.language, display);
  if (!state || state.kind === "loading") return null;
  const lines =
    state.kind === "failed"
      ? failedLines(t, state.reason)
      : state.kind === "note"
        ? [{ key: "note", left: -2, tone: "muted" as const, text: state.text }]
        : state.rows.map(({ label, line }) => ({
            ...line,
            text: `${label} ${line.text}`,
          }));
  return (
    <QuotaLines
      lines={lines}
      queriedAt={quota?.queriedAt ?? null}
      loading={loading}
      onRefresh={() => refresh()}
    />
  );
};

export default CodebuddyAccountQuotaFooter;
