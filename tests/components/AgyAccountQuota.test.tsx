import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import AgyAccountQuotaFooter from "@/components/AgyAccountQuotaFooter";
import SubscriptionQuotaFooter from "@/components/SubscriptionQuotaFooter";
import { AgyAccountQuota } from "@/components/settings/auth/AccountQuota";
import { RefreshAllQuotaButton } from "@/components/settings/auth/RefreshAllQuotaButton";
import { invalidateAgyLoginCaches } from "@/lib/query/subscription";
import { createTestQueryClient } from "../utils/testQueryClient";

const mocks = vi.hoisted(() => ({
  getQuota: vi.fn(),
  getAgyAccountQuota: vi.fn(),
  authGetStatus: vi.fn(),
}));

vi.mock("@/lib/api/subscription", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/lib/api/subscription")>();
  return {
    ...actual,
    subscriptionApi: {
      ...actual.subscriptionApi,
      getQuota: mocks.getQuota,
      getAgyAccountQuota: mocks.getAgyAccountQuota,
    },
  };
});

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    authApi: {
      ...actual.authApi,
      authGetStatus: mocks.authGetStatus,
    },
  };
});

const notFoundQuota = () => ({
  tool: "agy",
  credentialStatus: "not_found" as const,
  credentialMessage: null,
  success: false,
  tiers: [],
  extraUsage: null,
  error: null,
  queriedAt: null,
});

const validQuota = () => ({
  tool: "agy",
  credentialStatus: "valid" as const,
  credentialMessage: null,
  success: true,
  tiers: [{ name: "agy_gemini_five_hour", utilization: 20, resetsAt: null }],
  extraUsage: null,
  error: null,
  queriedAt: Date.now(),
});

describe("Antigravity quota anti-risk manual-only behavior", () => {
  beforeEach(() => {
    mocks.getQuota.mockReset();
    mocks.getAgyAccountQuota.mockReset();
    mocks.authGetStatus.mockReset();
    mocks.getQuota.mockResolvedValue(notFoundQuota());
    mocks.getAgyAccountQuota.mockResolvedValue(notFoundQuota());
    mocks.authGetStatus.mockResolvedValue({
      provider: "agy_oauth",
      authenticated: true,
      default_account_id: "acc-1",
      accounts: [],
    });
  });

  it("AgyAccountQuota mounts with manual=false and triggers manual=true only on ↻ click", async () => {
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <AgyAccountQuota accountId="acc-1" login="a@example.com" isCurrent />
      </QueryClientProvider>,
    );

    await waitFor(() => {
      expect(mocks.getAgyAccountQuota).toHaveBeenCalledWith("acc-1", false);
    });
    expect(mocks.getAgyAccountQuota).not.toHaveBeenCalledWith("acc-1", true);

    mocks.getAgyAccountQuota.mockResolvedValueOnce(validQuota());
    await userEvent.click(
      await screen.findByRole("button", { name: "刷新 a@example.com 的额度" }),
    );
    await waitFor(() => {
      expect(mocks.getAgyAccountQuota).toHaveBeenCalledWith("acc-1", true);
    });
  });

  it("AgyAccountQuotaFooter mounts with manual=false and triggers manual=true only on ↻ click", async () => {
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <AgyAccountQuotaFooter accountId="acc-1" inline isCurrent />
      </QueryClientProvider>,
    );

    await waitFor(() => {
      expect(mocks.getAgyAccountQuota).toHaveBeenCalledWith("acc-1", false);
    });
    expect(mocks.getAgyAccountQuota).not.toHaveBeenCalledWith("acc-1", true);

    mocks.getAgyAccountQuota.mockResolvedValueOnce(validQuota());
    await userEvent.click(
      await screen.findByRole("button", { name: /quota\.manualIdle/ }),
    );
    await waitFor(() => {
      expect(mocks.getAgyAccountQuota).toHaveBeenCalledWith("acc-1", true);
    });
  });

  it("SubscriptionQuotaFooter(appId='agy') mounts with manual=false and triggers manual=true only on ↻ click", async () => {
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <SubscriptionQuotaFooter appId="agy" inline isCurrent />
      </QueryClientProvider>,
    );

    await waitFor(() => {
      expect(mocks.getQuota).toHaveBeenCalledWith("agy", false);
    });
    expect(mocks.getQuota).not.toHaveBeenCalledWith("agy", true);

    mocks.getQuota.mockResolvedValueOnce(validQuota());
    await userEvent.click(
      await screen.findByRole("button", { name: /quota\.manualIdle/ }),
    );
    await waitFor(() => {
      expect(mocks.getQuota).toHaveBeenCalledWith("agy", true);
    });
  });

  it("RefreshAllQuotaButton sends manual=true, while invalidateAgyLoginCaches keeps manual=false", async () => {
    const client = createTestQueryClient();
    render(
      <QueryClientProvider client={client}>
        <RefreshAllQuotaButton />
        <AgyAccountQuota accountId="acc-1" login="a@example.com" isCurrent />
      </QueryClientProvider>,
    );

    await waitFor(() => {
      expect(mocks.getAgyAccountQuota).toHaveBeenCalledWith("acc-1", false);
    });

    await invalidateAgyLoginCaches(client);
    await waitFor(() => {
      expect(mocks.getAgyAccountQuota).toHaveBeenCalledTimes(2);
    });
    expect(mocks.getAgyAccountQuota).not.toHaveBeenCalledWith("acc-1", true);

    await userEvent.click(screen.getByRole("button", { name: "全部刷新" }));
    await waitFor(() => {
      expect(mocks.getAgyAccountQuota).toHaveBeenCalledWith("acc-1", true);
    });
  });
});
