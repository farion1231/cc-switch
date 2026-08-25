import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import {
  applyUsageCacheUpdate,
  type UsageCacheUpdatedPayload,
} from "@/hooks/useUsageCacheBridge";
import { subscriptionKeys } from "@/lib/query/subscription";
import type { SubscriptionQuota } from "@/types/subscription";

const quota: SubscriptionQuota = {
  tool: "codex",
  credentialStatus: "valid",
  credentialMessage: null,
  success: true,
  tiers: [],
  extraUsage: null,
  planType: "plus",
  error: null,
  queriedAt: 1,
};

describe("applyUsageCacheUpdate", () => {
  it("writes subscription events only to their exact provider scope", () => {
    const queryClient = new QueryClient();
    const payload: UsageCacheUpdatedPayload = {
      kind: "subscription",
      appType: "codex",
      scopeKey: "provider-a",
      data: quota,
    };

    applyUsageCacheUpdate(queryClient, payload);

    expect(
      queryClient.getQueryData(subscriptionKeys.quota("codex", "provider-a")),
    ).toEqual(quota);
    expect(
      queryClient.getQueryData(subscriptionKeys.quota("codex", "provider-b")),
    ).toBeUndefined();
  });
});
