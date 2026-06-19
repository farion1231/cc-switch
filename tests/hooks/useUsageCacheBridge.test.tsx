import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { subscriptionKeys } from "@/lib/query/subscription";
import type { SubscriptionQuota } from "@/types/subscription";

let eventHandler: ((payload: any) => void) | undefined;

vi.mock("@/hooks/useTauriEvent", () => ({
  useTauriEvent: (_event: string, handler: (payload: any) => void) => {
    eventHandler = handler;
  },
}));

import { useUsageCacheBridge } from "@/hooks/useUsageCacheBridge";

describe("useUsageCacheBridge", () => {
  beforeEach(() => {
    eventHandler = undefined;
  });

  it("keeps reset-credit snapshots in the enabled cache branch", () => {
    const queryClient = new QueryClient();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const quota: SubscriptionQuota = {
      tool: "codex",
      credentialStatus: "valid",
      credentialMessage: null,
      success: true,
      tiers: [],
      extraUsage: null,
      resetCredits: { availableCount: 1, credits: [] },
      error: null,
      queriedAt: 1,
    };

    renderHook(() => useUsageCacheBridge(), { wrapper });
    act(() => {
      eventHandler?.({
        kind: "subscription",
        appType: "codex",
        includeResetCredits: true,
        data: quota,
      });
    });

    expect(
      queryClient.getQueryData(subscriptionKeys.quota("codex", true)),
    ).toEqual(quota);
    expect(
      queryClient.getQueryData(subscriptionKeys.quota("codex", false)),
    ).toBeUndefined();
  });
});
