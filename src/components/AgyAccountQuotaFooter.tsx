import React from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useAgyAccountQuota } from "@/lib/query/subscription";
import { authGetStatus, type ManagedAuthStatus } from "@/lib/api/auth";
import { toast } from "@/lib/toast";
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
 * 这个号不是 agy 现在登着的号时，显示切走前最后一次查到的（没有就不显示）；这时点刷新只会
 * 读回同一份旧结果，另外提示只能查 agy 当前登着的号（和授权中心那一行同一句）。
 */
const AgyAccountQuotaFooter: React.FC<AgyAccountQuotaFooterProps> = ({
  accountId,
  inline = false,
  isCurrent = false,
  autoQueryInterval = 5,
}) => {
  const { t } = useTranslation();
  const display = useQuotaDisplay();
  // 和授权中心共用同一份账号状态缓存：default_account_id 就是 agy 现在登着的号
  const { data: authStatus } = useQuery<ManagedAuthStatus>({
    queryKey: ["managed-auth-status", "agy_oauth"],
    queryFn: () => authGetStatus("agy_oauth"),
    staleTime: 30000,
  });
  const notCurrent =
    authStatus != null && authStatus.default_account_id !== accountId;
  const {
    data: quota,
    isFetching: loading,
    refetch,
    manualIdle,
  } = useAgyAccountQuota(accountId, {
    enabled: true,
    autoQuery: isCurrent && autoQueryInterval > 0,
    autoQueryIntervalMinutes: autoQueryInterval,
  });

  return (
    <SubscriptionQuotaView
      quota={quota}
      loading={loading}
      refetch={() => {
        // 手动查询模式下第一次点仍要读回切走前的结果，所以照常查、原样返回（额度列靠它判断成败），只是另外说明
        const result = refetch();
        if (notCurrent) {
          toast.info(
            t("authCenter.agy.quotaRefreshNotCurrent", {
              defaultValue:
                "agy 只能查当前登着的号的额度，这里显示的是切走前最后一次查到的。切到这个号后再刷新。",
            }),
          );
        }
        return result;
      }}
      idle={manualIdle}
      appIdForExpiredHint="agy"
      inline={inline}
      display={display}
    />
  );
};

export default AgyAccountQuotaFooter;
