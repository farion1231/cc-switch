/** @fileoverview Subscription quota for a managed Kimi account. */

import { Loader2 } from "lucide-react";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";
import { useKimiOauthQuotaByAccountId } from "@/lib/query/subscription";

interface KimiOauthAccountQuotaProps {
  accountId: string;
}

/** Displays the expanded quota view for one Kimi account. */
export default function KimiOauthAccountQuota({
  accountId,
}: KimiOauthAccountQuotaProps) {
  const {
    data: quota,
    isFetching,
    refetch,
  } = useKimiOauthQuotaByAccountId(accountId, {
    enabled: true,
    autoQuery: false,
  });

  if (isFetching && !quota) {
    return (
      <div className="mt-3 flex items-center justify-center rounded-xl border border-border-default bg-card py-5 shadow-sm">
        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <SubscriptionQuotaView
      quota={quota}
      loading={isFetching}
      refetch={refetch}
      appIdForExpiredHint="kimi_oauth"
      inline={false}
    />
  );
}
