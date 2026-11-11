import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useClaudeAccountQuota } from "@/lib/query/subscription";

const mocks = vi.hoisted(() => ({ getClaudeAccountQuota: vi.fn() }));
vi.mock("@/lib/api/subscription", () => ({
  subscriptionApi: { getClaudeAccountQuota: mocks.getClaudeAccountQuota },
}));

const statusKey = ["managed-auth-status", "claude_official"];
const quota = (plan: unknown) => ({
  tool: "claude",
  credentialStatus: "valid",
  credentialMessage: null,
  success: true,
  tiers: [],
  extraUsage: null,
  error: null,
  queriedAt: 1,
  plan,
});

function setup(stored: { plan?: string; rate_limit_tier?: string }) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  client.setQueryData(statusKey, {
    provider: "claude_official",
    authenticated: true,
    default_account_id: null,
    accounts: [{ id: "acc:org", login: "me@example.com", ...stored }],
  });
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const hook = renderHook(() => useClaudeAccountQuota("acc:org"), { wrapper });
  return { hook, invalidate };
}

describe("useClaudeAccountQuota", () => {
  beforeEach(() => mocks.getClaudeAccountQuota.mockReset());

  it("refreshes the account list when the plan changed", async () => {
    mocks.getClaudeAccountQuota.mockResolvedValue(
      quota({ subscriptionType: "max", rateLimitTier: "default_claude_max_20x" }),
    );
    const { hook, invalidate } = setup({ plan: "pro" });
    await waitFor(() => expect(hook.result.current.isSuccess).toBe(true));
    expect(invalidate).toHaveBeenCalledWith({ queryKey: statusKey });
  });

  it("leaves the list alone when the plan is the same or unknown", async () => {
    mocks.getClaudeAccountQuota.mockResolvedValue(
      quota({ subscriptionType: "max", rateLimitTier: "default_claude_max_20x" }),
    );
    const same = setup({ plan: "max", rate_limit_tier: "default_claude_max_20x" });
    await waitFor(() => expect(same.hook.result.current.isSuccess).toBe(true));
    expect(same.invalidate).not.toHaveBeenCalled();

    mocks.getClaudeAccountQuota.mockResolvedValue(quota(undefined));
    const unknown = setup({ plan: "pro" });
    await waitFor(() =>
      expect(unknown.hook.result.current.isSuccess).toBe(true),
    );
    expect(unknown.invalidate).not.toHaveBeenCalled();
  });
});
