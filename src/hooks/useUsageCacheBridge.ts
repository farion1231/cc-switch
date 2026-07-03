import { useQueryClient } from "@tanstack/react-query";
import type { AppId } from "@/lib/api/types";
import type { UsageResult } from "@/types";
import type { SubscriptionQuota } from "@/types/subscription";
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
      kind: "script";
      appType: AppId;
      providerId: string;
      keyId: string;
      data: UsageResult;
    }
  | {
      kind: "subscription";
      appType: AppId;
      data: SubscriptionQuota;
    };

/**
 * 后端 `UsageCache` 写入后会 emit `usage-cache-updated`，本 hook 把 payload 同步到
 * React Query 缓存，让托盘触发的刷新（不经前端）也能立刻反映到主界面，避免
 * React Query 与 Rust 侧两份缓存各自为战。
 *
 * **kind === "script" 有两种形态**：
 *   - 没 keyId → provider-level 快照（`queryProviderUsage`），写到
 *     `usageKeys.script(providerId, appType)`，给 useUsageQuery / ProviderCard 用。
 *   - 有 keyId → per-key 快照（`queryProviderUsageForKey`），写到
 *     `["keyUsage", keyId, appId]`，给 ApiKeyListSection / KeyPoolList 用。
 * 两种必须分流——否则 per-key 的事件会把 provider-level 缓存污染成某一把
 * key 的数据，下次 useUsageQuery 读出来还是错的。
 */
export function useUsageCacheBridge() {
  const queryClient = useQueryClient();

  useTauriEvent<UsageCacheUpdatedPayload>("usage-cache-updated", (payload) => {
    if (payload.kind === "script") {
      if ("keyId" in payload && payload.keyId) {
        // per-key 快照——写到 keyId 维度的 cache。
        queryClient.setQueryData<UsageResult>(
          ["keyUsage", payload.keyId, payload.appType],
          payload.data,
        );
      } else {
        // provider-level 快照——legacy queryProviderUsage 路径。
        queryClient.setQueryData<UsageResult>(
          usageKeys.script(payload.providerId, payload.appType),
          payload.data,
        );
      }
    } else if (payload.kind === "subscription") {
      queryClient.setQueryData<SubscriptionQuota>(
        subscriptionKeys.quota(payload.appType),
        payload.data,
      );
    }
  });
}
