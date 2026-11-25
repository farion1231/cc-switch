import { QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  agyAuthStatusQuery,
  useAgyAccountQuota,
} from "@/lib/query/subscription";
import { createTestQueryClient } from "../utils/testQueryClient";

const mocks = vi.hoisted(() => ({ getAgyAccountQuota: vi.fn() }));
vi.mock("@/lib/api/subscription", () => ({
  subscriptionApi: { getAgyAccountQuota: mocks.getAgyAccountQuota },
}));

const quota = (overrides: Record<string, unknown> = {}) => ({
  tool: "agy",
  credentialStatus: "valid",
  credentialMessage: null,
  success: true,
  tiers: [],
  extraUsage: null,
  error: null,
  queriedAt: 1,
  ...overrides,
});

function setup(stored: {
  avatar_url?: string | null;
  requires_reauth?: boolean;
}) {
  const client = createTestQueryClient();
  client.setQueryData(["settings"], { officialQuotaManualOnly: false });
  client.setQueryData(agyAuthStatusQuery.queryKey, {
    provider: "agy_oauth",
    authenticated: true,
    default_account_id: "sub-1",
    accounts: [
      {
        id: "sub-1",
        provider: "agy_oauth",
        login: "alice@example.com",
        avatar_url: null,
        authenticated_at: 100,
        is_default: true,
        github_domain: "google.com",
        requires_reauth: false,
        ...stored,
      },
    ],
  });
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const hook = renderHook(() => useAgyAccountQuota("sub-1"), { wrapper });
  return { hook, invalidate };
}

describe("useAgyAccountQuota", () => {
  beforeEach(() => mocks.getAgyAccountQuota.mockReset());

  it("refreshes the account list when a quota check discovers reauth state changed", async () => {
    mocks.getAgyAccountQuota.mockResolvedValue(
      quota({
        credentialStatus: "expired",
        success: false,
        error: "Antigravity sign-in expired",
      }),
    );
    const expired = setup({
      avatar_url: "https://lh3.googleusercontent.com/a/public=s96-c",
      requires_reauth: false,
    });
    await waitFor(() =>
      expect(expired.hook.result.current.isSuccess).toBe(true),
    );
    expect(expired.invalidate).toHaveBeenCalledWith({
      queryKey: agyAuthStatusQuery.queryKey,
    });

    mocks.getAgyAccountQuota.mockResolvedValue(
      quota({
        avatarUrl: "https://lh3.googleusercontent.com/a/public=s96-c",
      }),
    );
    const recovered = setup({
      avatar_url: "https://lh3.googleusercontent.com/a/public=s96-c",
      requires_reauth: true,
    });
    await waitFor(() =>
      expect(recovered.hook.result.current.isSuccess).toBe(true),
    );
    expect(recovered.invalidate).toHaveBeenCalledWith({
      queryKey: agyAuthStatusQuery.queryKey,
    });
  });

  it("refreshes the account list when quota returns a backfilled or rotated avatarUrl", async () => {
    mocks.getAgyAccountQuota.mockResolvedValue(
      quota({
        avatarUrl: "https://lh3.googleusercontent.com/a-/private-1=s96-c",
      }),
    );
    const backfilled = setup({ avatar_url: null });
    await waitFor(() =>
      expect(backfilled.hook.result.current.isSuccess).toBe(true),
    );
    expect(backfilled.invalidate).toHaveBeenCalledWith({
      queryKey: agyAuthStatusQuery.queryKey,
    });

    mocks.getAgyAccountQuota.mockResolvedValue(
      quota({
        avatarUrl: "https://lh3.googleusercontent.com/a-/private-2=s96-c",
      }),
    );
    const rotated = setup({
      avatar_url: "https://lh3.googleusercontent.com/a-/private-1=s96-c",
    });
    await waitFor(() =>
      expect(rotated.hook.result.current.isSuccess).toBe(true),
    );
    expect(rotated.invalidate).toHaveBeenCalledWith({
      queryKey: agyAuthStatusQuery.queryKey,
    });
  });

  it("leaves the account list alone when reauth state and avatarUrl are unchanged", async () => {
    mocks.getAgyAccountQuota.mockResolvedValue(
      quota({
        avatarUrl: "https://lh3.googleusercontent.com/a-/private-1=s96-c",
      }),
    );
    const unchangedPrivate = setup({
      avatar_url: "https://lh3.googleusercontent.com/a-/private-1=s96-c",
      requires_reauth: false,
    });
    await waitFor(() =>
      expect(unchangedPrivate.hook.result.current.isSuccess).toBe(true),
    );
    expect(unchangedPrivate.invalidate).not.toHaveBeenCalled();

    mocks.getAgyAccountQuota.mockResolvedValue(quota());
    const stillMissing = setup({
      avatar_url: null,
      requires_reauth: false,
    });
    await waitFor(() =>
      expect(stillMissing.hook.result.current.isSuccess).toBe(true),
    );
    expect(stillMissing.invalidate).not.toHaveBeenCalled();
  });
});
