import React from "react";
import { useTranslation } from "react-i18next";
import { useCodexOauthQuotaByAccountId } from "@/lib/query/subscription";
import {
  AccountQuotaColumn,
  subscriptionQuotaState,
} from "@/components/settings/auth/AccountQuota";
import { useQuotaDisplay } from "@/components/quota/useQuotaDisplay";

interface CodexOauthAccountQuotaProps {
  /** cc-switch 自管的 ChatGPT 账号 ID */
  accountId: string;
  /** 账号名，拼进刷新按钮的无障碍名字 */
  login?: string;
}

/**
 * 授权中心里单个 ChatGPT (Codex OAuth) 账号的额度：账号行右侧的额度条 + 「x 分钟前 ↻」。
 *
 * 按 accountId 查 cc-switch 自管 OAuth token 的订阅额度（和绑定同一账号的供应商卡片共用缓存），
 * 打开页面时查一次，不轮询；点 ↻ 手动重查。
 */
const CodexOauthAccountQuota: React.FC<CodexOauthAccountQuotaProps> = ({
  accountId,
  login = "",
}) => {
  const { t, i18n } = useTranslation();
  const display = useQuotaDisplay();
  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useCodexOauthQuotaByAccountId(accountId, {
    enabled: true,
    autoQuery: false,
  });

  return (
    <AccountQuotaColumn
      login={login}
      state={subscriptionQuotaState(t, quota, loading, i18n.language, display)}
      queriedAt={quota?.queriedAt ?? null}
      loading={loading}
      onRefresh={() => void refetch()}
    />
  );
};

export default CodexOauthAccountQuota;
