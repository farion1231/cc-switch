import { useQueryClient } from "@tanstack/react-query";
import type { AppId } from "@/lib/api/types";
import type { UsageResult } from "@/types";
import type {
  CodexQuotaForecast,
  SubscriptionQuota,
} from "@/types/subscription";
import { usageKeys } from "@/lib/query/usage";
import { subscriptionKeys } from "@/lib/query/subscription";
import { useTauriEvent } from "./useTauriEvent";

type UsageCacheUpdatedPayload =
  | {
      kind: "script";
      appType: AppId;
      providerId: string;
      data: UsageResult;
    }
  | {
      kind: "subscription";
      appType: AppId;
      data: SubscriptionQuota;
    };

type CodexAccountQuotasPayload = {
  kind: "codex-all";
  accounts: Array<{ accountKey: string; quota: SubscriptionQuota }>;
  forecasts?: Record<string, CodexQuotaForecast>;
};

/**
 * 后端 `UsageCache` 写入后会 emit `usage-cache-updated`，本 hook 把 payload 同步到
 * React Query 缓存，让托盘触发的刷新（不经前端）也能立刻反映到主界面，避免
 * React Query 与 Rust 侧两份缓存各自为战。
 */
export function useUsageCacheBridge() {
  const queryClient = useQueryClient();

  useTauriEvent<UsageCacheUpdatedPayload>("usage-cache-updated", (payload) => {
    if (payload.kind === "script") {
      queryClient.setQueryData<UsageResult>(
        usageKeys.script(payload.providerId, payload.appType),
        payload.data,
      );
    } else if (payload.kind === "subscription") {
      queryClient.setQueryData<SubscriptionQuota>(
        subscriptionKeys.quota(payload.appType),
        payload.data,
      );
    }
  });

  useTauriEvent<CodexAccountQuotasPayload>(
    "codex-account-quotas-updated",
    ({ accounts, forecasts }) => {
      const quotas = Object.fromEntries(
        accounts.map(({ accountKey, quota }) => [accountKey, quota]),
      );
      queryClient.setQueryData<Record<string, SubscriptionQuota>>(
        subscriptionKeys.allCodexQuotas(),
        quotas,
      );
      if (forecasts) {
        queryClient.setQueryData<Record<string, CodexQuotaForecast>>(
          subscriptionKeys.codexForecasts(),
          forecasts,
        );
      }
    },
  );
}
