import { useQuery } from "@tanstack/react-query";
import {
  copilotGetUsage,
  copilotGetUsageForAccount,
  type QuotaDetail,
} from "@/lib/api/copilot";
import type { QuotaTier } from "@/types/subscription";

const REFETCH_INTERVAL = 5 * 60 * 1000; // 5 minutes

/** 一项 Copilot 额度，带上次数：面板只画百分比，总额写进悬停说明 */
export interface CopilotQuotaTier extends QuotaTier {
  name: CopilotQuotaKind;
  remaining: number;
  entitlement: number;
  /** false：当前计划不含这一项（Free 的 Credits），不是用完了 */
  included: boolean;
}

export type CopilotQuotaKind = "premium" | "completions" | "chat";

export interface CopilotQuota {
  success: boolean;
  plan: string | null;
  resetDate: string | null;
  tiers: CopilotQuotaTier[];
  error: string | null;
  queriedAt: number | null;
}

export interface UseCopilotQuotaOptions {
  enabled?: boolean;
  /** 是否启用自动轮询（5 分钟）与窗口 focus 重取 */
  autoQuery?: boolean;
}

export function useCopilotQuota(
  accountId: string | null,
  options: UseCopilotQuotaOptions = {},
) {
  const { enabled = true, autoQuery = false } = options;
  return useQuery<CopilotQuota>({
    queryKey: ["copilot", "quota", accountId ?? "default"],
    queryFn: async (): Promise<CopilotQuota> => {
      const usage = accountId
        ? await copilotGetUsageForAccount(accountId)
        : await copilotGetUsage();

      const snapshots = usage.quota_snapshots;
      // 付费计划的补全、聊天标成 unlimited，不显示；Free 的补全、聊天有每月上限，要显示。
      // Free 没有 Credits（has_quota=false、总额 0）：留一行写「不含」，免得两个号的行对不上
      const entries: [CopilotQuotaKind, QuotaDetail | undefined][] = [
        ["premium", snapshots.premium_interactions],
        ["completions", snapshots.completions],
        ["chat", snapshots.chat],
      ];
      const tiers = entries.flatMap(([name, detail]): CopilotQuotaTier[] => {
        if (!detail || detail.unlimited) return [];
        const base = {
          name,
          resetsAt: usage.quota_reset_date,
          remaining: detail.remaining,
          entitlement: detail.entitlement,
        };
        if (detail.has_quota === false) {
          return [{ ...base, utilization: 0, included: false }];
        }
        if (detail.entitlement <= 0) return [];
        return [
          {
            ...base,
            utilization:
              ((detail.entitlement - detail.remaining) / detail.entitlement) *
              100,
            included: true,
          },
        ];
      });

      return {
        success: true,
        plan: usage.copilot_plan,
        resetDate: usage.quota_reset_date,
        tiers,
        error: null,
        queriedAt: Date.now(),
      };
    },
    enabled,
    refetchInterval: autoQuery ? REFETCH_INTERVAL : false,
    refetchIntervalInBackground: autoQuery,
    refetchOnWindowFocus: autoQuery,
    staleTime: REFETCH_INTERVAL,
    retry: 1,
  });
}
