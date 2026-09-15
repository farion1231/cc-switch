import React from "react";
import type { ProviderMeta } from "@/types";
import { useXaiOauthQuota } from "@/lib/query/subscription";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";

interface XaiOauthQuotaFooterProps {
  meta?: ProviderMeta;
  inline?: boolean;
  /** 是否为当前激活的供应商 */
  isCurrent?: boolean;
}

/**
 * xAI OAuth (SuperGrok 反代) 订阅额度 footer
 *
 * 复用 SubscriptionQuotaView 的全部渲染逻辑（5 状态 × inline/expanded）。
 * 数据源为 cc-switch 自管的 xAI OAuth token，与 Grok Build 分区读 Grok CLI
 * 凭据的路径查同一个 grok.com 账单端点，展示同一份订阅额度。
 */
const XaiOauthQuotaFooter: React.FC<XaiOauthQuotaFooterProps> = ({
  meta,
  inline = false,
  isCurrent = false,
}) => {
  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useXaiOauthQuota(meta, { enabled: true, autoQuery: isCurrent });

  return (
    <SubscriptionQuotaView
      quota={quota}
      loading={loading}
      refetch={refetch}
      appIdForExpiredHint="xai_oauth"
      inline={inline}
    />
  );
};

export default XaiOauthQuotaFooter;
