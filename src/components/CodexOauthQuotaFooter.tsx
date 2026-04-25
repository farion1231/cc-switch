import React from "react";
import type { ProviderMeta } from "@/types";
import {
  useCodexOauthQuota,
  useCodexOauthQuotaByAccountId,
} from "@/lib/query/subscription";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";

interface CodexOauthQuotaFooterProps {
  meta?: ProviderMeta;
  accountId?: string | null;
  inline?: boolean;
  /** 是否为当前激活的供应商 */
  isCurrent?: boolean;
}

/**
 * Codex OAuth (ChatGPT Plus/Pro 反代) 订阅额度 footer
 *
 * 复用 SubscriptionQuotaView 的全部渲染逻辑（5 状态 × inline/expanded）。
 * 数据源切换为 cc-switch 自管的 OAuth token 而非 Codex CLI 凭据。
 */
const CodexOauthQuotaFooter: React.FC<CodexOauthQuotaFooterProps> = ({
  meta,
  accountId,
  inline = false,
}) => {
  const hasExplicitAccount = accountId !== undefined;
  const hasMetaBinding = meta !== undefined;

  const accountQuery = useCodexOauthQuotaByAccountId(accountId, {
    enabled: hasExplicitAccount,
    autoQuery: hasExplicitAccount,
  });
  const metaQuery = useCodexOauthQuota(meta, {
    enabled: !hasExplicitAccount && hasMetaBinding,
    autoQuery: !hasExplicitAccount && hasMetaBinding,
  });

  const query = hasExplicitAccount ? accountQuery : metaQuery;
  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = query;

  return (
    <SubscriptionQuotaView
      quota={quota}
      loading={loading}
      refetch={refetch}
      appIdForExpiredHint="codex_oauth"
      inline={inline}
    />
  );
};

export default CodexOauthQuotaFooter;
