import React from "react";
import { useQuery } from "@tanstack/react-query";
import { authApi } from "@/lib/api";
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
  // 和授权中心同一份查询。WSL 里的额度还没接到发行版内的 Claude Code，先不查，免得报错。
  const loginSwitch = useQuery({
    queryKey: ["claude-login-switch"],
    queryFn: () => authApi.getClaudeLoginSwitch(),
    staleTime: 30000,
  });
  const wslAccounts = loginSwitch.data?.wslAccounts === true;
  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useClaudeAccountQuota(accountId, {
    enabled: !loginSwitch.isPending && !wslAccounts,
    autoQuery: isCurrent && autoQueryInterval > 0 && !wslAccounts,
    autoQueryIntervalMinutes: autoQueryInterval,
  });
  if (wslAccounts) return null;

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
