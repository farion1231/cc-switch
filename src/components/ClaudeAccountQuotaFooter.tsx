import React from "react";
import { useClaudeAccountQuota } from "@/lib/query/subscription";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";
import { useQuotaDisplay } from "@/components/quota/useQuotaDisplay";

interface ClaudeAccountQuotaFooterProps {
  /** 卡片绑定的 Claude 官方账号 */
  accountId: string;
  inline?: boolean;
  isCurrent?: boolean;
  autoQueryInterval?: number;
}

/**
 * 绑了账号的 Claude 官方卡额度：每张卡按自己的号查，不只是正在用的那张。
 * 正在用的那张按设置的间隔自动刷新。
 */
const ClaudeAccountQuotaFooter: React.FC<ClaudeAccountQuotaFooterProps> = ({
  accountId,
  inline = false,
  isCurrent = false,
  autoQueryInterval = 5,
}) => {
  const display = useQuotaDisplay();
  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useClaudeAccountQuota(accountId, {
    autoQuery: isCurrent && autoQueryInterval > 0,
    autoQueryIntervalMinutes: autoQueryInterval,
  });

  return (
    <SubscriptionQuotaView
      quota={quota}
      loading={loading}
      refetch={refetch}
      appIdForExpiredHint="claude"
      inline={inline}
      display={display}
    />
  );
};

export default ClaudeAccountQuotaFooter;
