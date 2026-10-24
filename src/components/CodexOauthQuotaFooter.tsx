import React from "react";
import type { ProviderMeta } from "@/types";
import { useQueryClient } from "@tanstack/react-query";
import {
  forceRefreshCodexResets,
  useCodexOauthQuota,
} from "@/lib/query/subscription";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";
import { useQuotaDisplay } from "@/components/quota/useQuotaDisplay";

interface CodexOauthQuotaFooterProps {
  meta?: ProviderMeta;
  inline?: boolean;
  /** 是否为当前激活的供应商 */
  isCurrent?: boolean;
  autoQueryInterval?: number;
}

/**
 * Codex OAuth (ChatGPT Plus/Pro 反代) 订阅额度 footer
 *
 * 复用 SubscriptionQuotaView 的全部渲染逻辑（5 状态 × inline/expanded）。
 * 数据源切换为 cc-switch 自管的 OAuth token 而非 Codex CLI 凭据。
 * 点 ↻ 刷新额度时顺带强制刷新一次社区重置预测（见 forceRefreshCodexResets）。
 */
const CodexOauthQuotaFooter: React.FC<CodexOauthQuotaFooterProps> = ({
  meta,
  inline = false,
  isCurrent = false,
  autoQueryInterval = 5,
}) => {
  const display = useQuotaDisplay();
  const queryClient = useQueryClient();
  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useCodexOauthQuota(meta, {
    enabled: true,
    autoQuery: isCurrent && autoQueryInterval > 0,
    autoQueryIntervalMinutes: autoQueryInterval,
  });

  const handleRefresh = React.useCallback(() => {
    void forceRefreshCodexResets(queryClient);
    return refetch();
  }, [queryClient, refetch]);

  return (
    <SubscriptionQuotaView
      quota={quota}
      loading={loading}
      refetch={handleRefresh}
      appIdForExpiredHint="codex_oauth"
      inline={inline}
      display={display}
    />
  );
};

export default CodexOauthQuotaFooter;
