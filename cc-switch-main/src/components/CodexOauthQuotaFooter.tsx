import React from "react";
import type { ProviderMeta } from "@/types";
import { useCodexOauthQuota } from "@/lib/query/subscription";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";

interface CodexOauthQuotaFooterProps {
  meta?: ProviderMeta;
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
  inline = false,
  isCurrent = false,
}) => {
  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useCodexOauthQuota(meta, { enabled: true, autoQuery: isCurrent });

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
