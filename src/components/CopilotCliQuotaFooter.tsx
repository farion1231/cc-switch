import React from "react";
import { useTranslation } from "react-i18next";
import type { ProviderMeta } from "@/types";
import CopilotQuotaFooter from "@/components/CopilotQuotaFooter";
import { QuotaLines } from "@/components/quota/QuotaLines";
import { failedLines } from "@/components/quota/quotaRules";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { useCopilotCliSignedInAccount } from "@/lib/query/copilot";

interface CopilotCliQuotaFooterProps {
  meta?: ProviderMeta;
  isCurrent?: boolean;
}

/**
 * GitHub Copilot CLI 官方卡的 Premium 额度：绑了授权中心账号就查那个账号，没绑时跟着
 * Copilot CLI 当前登录的账号（在授权中心里按用户名和域名找）。
 */
const CopilotCliQuotaFooter: React.FC<CopilotCliQuotaFooterProps> = ({
  meta,
  isCurrent = false,
}) => {
  const { t } = useTranslation();
  const boundAccountId = resolveManagedAccountId(meta, "github_copilot");
  const {
    data: signedIn,
    isFetching,
    isPending,
    refetch,
  } = useCopilotCliSignedInAccount(!boundAccountId);

  if (boundAccountId) {
    return (
      <CopilotQuotaFooter
        meta={meta}
        inline={true}
        isCurrent={isCurrent}
        accountId={boundAccountId}
      />
    );
  }
  if (isPending) return null;
  if (signedIn?.accountId) {
    return (
      <CopilotQuotaFooter
        meta={meta}
        inline={true}
        isCurrent={isCurrent}
        accountId={signedIn.accountId}
      />
    );
  }
  const reason = signedIn
    ? t("copilot.quota.accountNotInAuthCenter", { login: signedIn.login })
    : t("copilot.quota.notSignedIn");
  return (
    <QuotaLines
      lines={failedLines(t, reason)}
      loading={isFetching}
      onRefresh={refetch}
    />
  );
};

export default CopilotCliQuotaFooter;
