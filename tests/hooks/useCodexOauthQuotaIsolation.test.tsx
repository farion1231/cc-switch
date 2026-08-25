import type { PropsWithChildren } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { subscriptionApi } from "@/lib/api/subscription";
import {
  subscriptionKeys,
  useCodexOauthQuotaByAccountId,
} from "@/lib/query/subscription";
import type { SubscriptionQuota } from "@/types/subscription";
import { createTestQueryClient } from "../utils/testQueryClient";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

const quota = (account: string): SubscriptionQuota => ({
  tool: account,
  credentialStatus: "valid",
  credentialMessage: null,
  success: true,
  tiers: [],
  extraUsage: null,
  planType: account,
  rateLimitResetCredits: { availableCount: 1, credits: null },
  error: null,
  queriedAt: Date.now(),
});

describe("Codex OAuth quota account isolation", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("does not let a late account A response overwrite account B", async () => {
    const accountA = deferred<SubscriptionQuota>();
    const accountB = deferred<SubscriptionQuota>();
    vi.spyOn(subscriptionApi, "getCodexOauthQuota").mockImplementation(
      (accountId) => {
        if (accountId === "account-a") return accountA.promise;
        if (accountId === "account-b") return accountB.promise;
        throw new Error(`unexpected account: ${accountId}`);
      },
    );

    const queryClient = createTestQueryClient();
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result, rerender } = renderHook(
      ({ accountId }) =>
        useCodexOauthQuotaByAccountId(accountId, { enabled: true }),
      { initialProps: { accountId: "account-a" }, wrapper },
    );

    rerender({ accountId: "account-b" });
    await act(async () => accountB.resolve(quota("account-b")));
    await waitFor(() => expect(result.current.data?.tool).toBe("account-b"));

    await act(async () => accountA.resolve(quota("account-a")));
    expect(result.current.data?.tool).toBe("account-b");
    expect(
      queryClient.getQueryData(["codex_oauth", "quota", "account-a"]),
    ).toMatchObject({ tool: "account-a" });
    expect(
      queryClient.getQueryData(["codex_oauth", "quota", "account-b"]),
    ).toMatchObject({ tool: "account-b" });
  });

  it("does not resolve an unbound card through a mutable default account", () => {
    const getQuota = vi.spyOn(subscriptionApi, "getCodexOauthQuota");
    const queryClient = createTestQueryClient();
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    const { result } = renderHook(
      () => useCodexOauthQuotaByAccountId(null, { enabled: true }),
      { wrapper },
    );

    expect(result.current.fetchStatus).toBe("idle");
    expect(getQuota).not.toHaveBeenCalled();
  });

  it("scopes CLI subscription keys by provider identity", () => {
    expect(subscriptionKeys.quota("codex", "provider-a")).not.toEqual(
      subscriptionKeys.quota("codex", "provider-b"),
    );
  });
});
