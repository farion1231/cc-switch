import React from "react";
import { useAgyAccountQuota } from "@/lib/query/subscription";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";
import { useQuotaDisplay } from "@/components/quota/useQuotaDisplay";

interface AgyAccountQuotaFooterProps {
  accountId: string;
  inline?: boolean;
  /** 是否为当前激活的供应商 */
  isCurrent?: boolean;
  autoQueryInterval?: number;
}

/**
 * 绑了授权中心账号的 Antigravity 官方卡：显示绑定那个号的额度（不是 agy 现在登着的号），
 * 和授权中心那一行共用同一份查询。
 */
const AgyAccountQuotaFooter: React.FC<AgyAccountQuotaFooterProps> = ({
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
  } = useAgyAccountQuota(accountId, {
    enabled: true,
    autoQuery: isCurrent && autoQueryInterval > 0,
    autoQueryIntervalMinutes: autoQueryInterval,
  });

  return (
    <SubscriptionQuotaView
      quota={quota}
      loading={loading}
      refetch={refetch}
      appIdForExpiredHint="agy"
      inline={inline}
      display={display}
    />
  );
};

export default AgyAccountQuotaFooter;
