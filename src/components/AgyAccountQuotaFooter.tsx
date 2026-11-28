import React from "react";
import { useQuery } from "@tanstack/react-query";
import {
  agyAuthStatusQuery,
  useAgyAccountQuota,
} from "@/lib/query/subscription";
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
 * 绑了授权中心账号的 Antigravity 官方卡：显示绑定那个号的额度，和授权中心那一行共用同一份查询。
 * 默认仅读本地 `last_quota` 缓存，绝不自动启动 `agy --print /usage`；仅在用户点 `↻` 时手动刷新。
 */
const AgyAccountQuotaFooter: React.FC<AgyAccountQuotaFooterProps> = ({
  accountId,
  inline = false,
}) => {
  const display = useQuotaDisplay();
  const { data: authStatus } = useQuery(agyAuthStatusQuery);
  const isActiveAccount = authStatus?.default_account_id === accountId;
  const {
    data: quota,
    isFetching: loading,
    refresh,
  } = useAgyAccountQuota(accountId, {
    enabled: true,
  });
  const hasCachedQuota =
    Boolean(quota) && quota?.credentialStatus !== "not_found";

  return (
    <SubscriptionQuotaView
      quota={hasCachedQuota ? quota : undefined}
      loading={loading}
      refetch={refresh}
      idle={isActiveAccount && !hasCachedQuota}
      appIdForExpiredHint="agy"
      inline={inline}
      display={display}
    />
  );
};

export default AgyAccountQuotaFooter;
