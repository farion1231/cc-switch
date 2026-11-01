import { render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import KimiAccountQuotaFooter from "@/components/KimiAccountQuotaFooter";
import { kimiOAuthApi } from "@/lib/api/kimiOAuth";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/lib/api/kimiOAuth", () => ({
  kimiOAuthApi: { status: vi.fn(), quota: vi.fn() },
}));

const account = {
  id: "kimi-official-b",
  name: "Kimi Official · Nick",
  authenticated: true,
  region: "mainland-cn" as const,
  expiresAt: null,
  error: null,
  nickname: "Nick",
  avatarUrl: null,
  primary: false,
};

function renderFooter(name: string, inline = false) {
  const client = createTestQueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  render(
    <QueryClientProvider client={client}>
      <KimiAccountQuotaFooter
        accountId={account.id}
        name={name}
        inline={inline}
      />
    </QueryClientProvider>,
  );
  return invalidate;
}

describe("KimiAccountQuotaFooter", () => {
  beforeEach(() => {
    vi.mocked(kimiOAuthApi.quota).mockResolvedValue({
      tool: "kimi",
      credentialStatus: "not_found",
      credentialMessage: null,
      success: false,
      tiers: [],
      extraUsage: null,
      error: null,
      queriedAt: null,
    });
    vi.mocked(kimiOAuthApi.status).mockResolvedValue({
      accounts: [account],
      currentAccountId: null,
      primaryHidden: false,
    });
  });

  it("reloads the card list once the account's nickname is known", async () => {
    const invalidate = renderFooter("Kimi Official (kimi-off)");
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: ["providers", "kimi"],
      }),
    );
  });

  it("leaves the list alone when the card already shows the nickname", async () => {
    const invalidate = renderFooter(account.name);
    await waitFor(() => expect(kimiOAuthApi.status).toHaveBeenCalled());
    expect(invalidate).not.toHaveBeenCalledWith({
      queryKey: ["providers", "kimi"],
    });
  });

  it("says the account has no subscription when the quota comes back empty", async () => {
    vi.mocked(kimiOAuthApi.quota).mockResolvedValue({
      tool: "kimi_oauth",
      credentialStatus: "valid",
      credentialMessage: null,
      success: true,
      tiers: [],
      extraUsage: null,
      error: null,
      queriedAt: 0,
    });
    renderFooter(account.name, true);
    expect(
      await screen.findByText("kimi.oauth.noSubscription"),
    ).toBeInTheDocument();
  });

  it("stays silent when the account has no login on this device", async () => {
    renderFooter(account.name, true);
    await waitFor(() => expect(kimiOAuthApi.quota).toHaveBeenCalled());
    expect(
      screen.queryByText("kimi.oauth.noSubscription"),
    ).not.toBeInTheDocument();
  });
});
