import React from "react";
import { useHermesCodexQuota } from "@/lib/query/subscription";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";

interface HermesCodexQuotaFooterProps {
  inline?: boolean;
  /** 是否为当前激活的供应商 */
  isCurrent?: boolean;
}

const HermesCodexQuotaFooter: React.FC<HermesCodexQuotaFooterProps> = ({
  inline = false,
  isCurrent = false,
}) => {
  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useHermesCodexQuota(true, isCurrent);

  return (
    <SubscriptionQuotaView
      quota={quota}
      loading={loading}
      refetch={refetch}
      appIdForExpiredHint="hermes openai-codex"
      inline={inline}
    />
  );
};

export default HermesCodexQuotaFooter;
