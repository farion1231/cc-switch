import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import type { AppId } from "@/lib/api/types";
import type { UsageResult } from "@/types";
import type { SubscriptionQuota } from "@/types/subscription";
import { usageKeys } from "@/lib/query/usage";
import { subscriptionKeys } from "@/lib/query/subscription";
import { useTauriEvent } from "./useTauriEvent";

export type UsageCacheUpdatedPayload =
  | {
      kind: "script";
      appType: AppId;
      providerId: string;
      data: UsageResult;
    }
  | {
      kind: "subscription";
      appType: AppId;
      scopeKey: string;
      data: SubscriptionQuota;
    };

export function applyUsageCacheUpdate(
  queryClient: QueryClient,
  payload: UsageCacheUpdatedPayload,
) {
  if (payload.kind === "script") {
    queryClient.setQueryData<UsageResult>(
      usageKeys.script(payload.providerId, payload.appType),
      payload.data,
    );
    return;
  }

  queryClient.setQueryData<SubscriptionQuota>(
    subscriptionKeys.quota(payload.appType, payload.scopeKey),
    payload.data,
  );
}

/**
 * 后端 `UsageCache` 写入后会 emit `usage-cache-updated`，本 hook 把 payload 同步到
 * React Query 缓存，让托盘触发的刷新（不经前端）也能立刻反映到主界面，避免
 * React Query 与 Rust 侧两份缓存各自为战。
 */
export function useUsageCacheBridge() {
  const queryClient = useQueryClient();

  useTauriEvent<UsageCacheUpdatedPayload>("usage-cache-updated", (payload) => {
    applyUsageCacheUpdate(queryClient, payload);
  });
}
