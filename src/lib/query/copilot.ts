import { useQuery } from "@tanstack/react-query";
import { copilotGetUsage, copilotGetUsageForAccount } from "@/lib/api/copilot";
import type { QuotaTier } from "@/types/subscription";

const REFETCH_INTERVAL = 5 * 60 * 1000; // 5 minutes

export interface CopilotQuota {
  success: boolean;
  plan: string | null;
  resetDate: string | null;
  tiers: QuotaTier[];
  error: string | null;
  queriedAt: number | null;
}

export function useCopilotQuota(accountId: string | null, enabled: boolean) {
  return useQuery<CopilotQuota>({
    queryKey: ["copilot", "quota", accountId ?? "default"],
    queryFn: async (): Promise<CopilotQuota> => {
      const usage = accountId
        ? await copilotGetUsageForAccount(accountId)
        : await copilotGetUsage();

      const premium = usage.quota_snapshots.premium_interactions;
      const utilization =
        premium.entitlement > 0
          ? ((premium.entitlement - premium.remaining) / premium.entitlement) *
            100
          : 0;

      return {
        success: true,
        plan: usage.copilot_plan,
        resetDate: usage.quota_reset_date,
        tiers: [
          {
            name: "premium",
            utilization,
            resetsAt: usage.quota_reset_date,
          },
        ],
        error: null,
        queriedAt: Date.now(),
      };
    },
    enabled,
    refetchInterval: REFETCH_INTERVAL,
    refetchOnWindowFocus: true,
    staleTime: REFETCH_INTERVAL,
    retry: 1,
  });
}
