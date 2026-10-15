import { useQueryClient } from "@tanstack/react-query";
import type { AppId } from "@/lib/api/types";
import type { UsageResult } from "@/types";
import type { SubscriptionQuota } from "@/types/subscription";
import { usageKeys } from "@/lib/query/usage";
import { subscriptionKeys } from "@/lib/query/subscription";
import {
  isTransientUsageError,
  resolveDisplayUsage,
} from "@/lib/query/queries";
import { useTauriEvent } from "./useTauriEvent";

type UsageCacheUpdatedPayload =
  | {
      kind: "codex_oauth";
      accountId: string;
      data: SubscriptionQuota;
    }
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

/**
 * 后端 `UsageCache` 写入后会 emit `usage-cache-updated`，本 hook 把 payload 同步到
 * React Query 缓存，让托盘触发的刷新（不经前端）也能立刻反映到主界面，避免
 * React Query 与 Rust 侧两份缓存各自为战。
 */
export function useUsageCacheBridge() {
  const queryClient = useQueryClient();

  useTauriEvent<UsageCacheUpdatedPayload>("usage-cache-updated", (payload) => {
    if (payload.kind === "codex_oauth") {
      const key = ["codex_oauth", "quota", payload.accountId];
      const previous = queryClient.getQueryState<SubscriptionQuota>(key);
      // Keep the shared snapshot even with no account/card mounted. Hook-local
      // refs cannot recover it after a background failure replaces the cache.
      if (
        previous?.data?.success &&
        payload.data.credentialStatus === "valid" &&
        isTransientUsageError(payload.data)
      ) {
        const display = resolveDisplayUsage(
          payload.data,
          Date.now(),
          { data: previous.data, at: previous.dataUpdatedAt },
          Date.now(),
        );
        if (display.data === previous.data) {
          // Do not renew dataUpdatedAt or trigger requests for every account.
          // A later mount can retry this stale snapshot normally.
          void queryClient.invalidateQueries({
            queryKey: key,
            exact: true,
            refetchType: "none",
          });
          return;
        }
      }
      queryClient.setQueryData<SubscriptionQuota>(key, payload.data);
    } else if (payload.kind === "script") {
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
}
