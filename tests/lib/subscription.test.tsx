/** @fileoverview Tests for account-scoped subscription queries. */

import type { PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useKimiOauthQuotaByAccountId } from "@/lib/query/subscription";

const mocks = vi.hoisted(() => ({
  getKimiOauthQuota: vi.fn(),
}));

vi.mock("@/lib/api/subscription", () => ({
  getKimiOauthQuota: mocks.getKimiOauthQuota,
  subscriptionApi: {
    getQuota: vi.fn(),
    getCodexOauthQuota: vi.fn(),
    getXaiOauthQuota: vi.fn(),
  },
}));

describe("Kimi OAuth subscription queries", () => {
  beforeEach(() => {
    mocks.getKimiOauthQuota.mockReset();
    mocks.getKimiOauthQuota.mockResolvedValue({
      tool: "kimi_oauth",
      credentialStatus: "valid",
      credentialMessage: null,
      success: true,
      tiers: [],
      extraUsage: null,
      error: null,
      queriedAt: 1_700_000_000_000,
    });
  });

  it("uses the account id as the cache key and command argument", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    const hook = renderHook(
      () => useKimiOauthQuotaByAccountId("kimi-account"),
      { wrapper },
    );
    await waitFor(() => expect(hook.result.current.data).toBeDefined());

    expect(mocks.getKimiOauthQuota).toHaveBeenCalledWith("kimi-account");
    expect(
      queryClient.getQueryData(["kimi_oauth", "quota", "kimi-account"]),
    ).toBeDefined();
  });
});
