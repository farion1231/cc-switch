import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useKimiAccountQuota } from "@/lib/query/subscription";
import { kimiOAuthApi } from "@/lib/api/kimiOAuth";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";
import { useQuotaDisplay } from "@/components/quota/useQuotaDisplay";
import { KIMI_OAUTH_STATUS_KEY } from "@/components/providers/forms/hooks/useKimiOAuth";

interface KimiAccountQuotaFooterProps {
  /** Kimi 官方账号的供应商 ID（授权中心里的账号 ID 与它相同） */
  accountId: string;
  /** 卡片当前显示的名字，用来判断昵称是否已经补上 */
  name?: string;
  inline?: boolean;
  isCurrent?: boolean;
}

/**
 * Kimi 官方账号卡的额度：每张卡就是一个账号，和授权中心那一行共用同一份查询。
 * 正在用的那张卡每 5 分钟自动刷新一次。
 */
const KimiAccountQuotaFooter: React.FC<KimiAccountQuotaFooterProps> = ({
  accountId,
  name,
  inline = false,
  isCurrent = false,
}) => {
  const { t } = useTranslation();
  const display = useQuotaDisplay();
  const queryClient = useQueryClient();
  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useKimiAccountQuota(accountId, {
    enabled: true,
    autoQuery: isCurrent,
  });

  // 卡片名里的昵称来自账号身份缓存，而缓存要查过账号状态才有。没进过授权中心时
  // 这里顺带查一次，查到昵称后重新拉供应商列表，卡片就能显示是哪个号。
  const { data: status } = useQuery({
    queryKey: KIMI_OAUTH_STATUS_KEY,
    queryFn: kimiOAuthApi.status,
    staleTime: 5 * 60_000,
  });
  const nickname = status?.accounts.find(
    (account) => account.id === accountId,
  )?.nickname;
  useEffect(() => {
    if (nickname && name && !name.includes(nickname)) {
      void queryClient.invalidateQueries({ queryKey: ["providers", "kimi"] });
    }
  }, [nickname, name, queryClient]);

  return (
    <SubscriptionQuotaView
      quota={quota}
      loading={loading}
      refetch={refetch}
      appIdForExpiredHint="kimi"
      inline={inline}
      display={display}
      emptyText={t("kimi.oauth.noSubscription")}
    />
  );
};

export default KimiAccountQuotaFooter;
