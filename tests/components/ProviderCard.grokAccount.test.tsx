import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { ManagedAuthStatus } from "@/lib/api";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const xaiQuotaFooterProps = vi.hoisted(() => vi.fn());

vi.mock("@/components/providers/ProviderCardActions", () => ({
  ProviderCardActions: () => null,
}));

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => null,
}));

vi.mock("@/components/UsageFooter", () => ({ default: () => null }));
vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: () => null,
}));
vi.mock("@/components/CopilotQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/CodexOauthQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/XaiOauthQuotaFooter", () => ({
  default: (props: unknown) => {
    xaiQuotaFooterProps(props);
    return <div>xai-oauth-quota</div>;
  },
}));

vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: undefined }),
}));

const grokOfficialProvider = (
  name: string,
  accountId?: string,
): Provider => ({
  id: accountId ? `grokbuild-official-${accountId}` : "grokbuild-official",
  name,
  category: "official",
  settingsConfig: { config: "" },
  meta: accountId
    ? {
        authBinding: {
          source: "managed_account",
          authProvider: "xai_oauth",
          accountId,
        },
      }
    : {},
});

const xaiAuthStatus = (login: string, accountId = "xai-1"): ManagedAuthStatus => ({
  provider: "xai_oauth",
  authenticated: true,
  default_account_id: accountId,
  accounts: [
    {
      id: accountId,
      provider: "xai_oauth",
      login,
      avatar_url: null,
      authenticated_at: 0,
      is_default: true,
      github_domain: "x.ai",
      reauth_required: false,
      requires_reauth: false,
    },
  ],
});

function renderCard(
  provider: Provider,
  options: {
    status?: ManagedAuthStatus;
    isCurrent?: boolean;
  } = {},
) {
  const queryClient = createTestQueryClient();
  if (options.status) {
    queryClient.setQueryData(
      ["managed-auth-status", "xai_oauth"],
      options.status,
    );
  }

  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderCard
        provider={provider}
        appId="grokbuild"
        isCurrent={options.isCurrent ?? false}
        presentation={{ chips: [], buttons: [] }}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onConfigureUsage={vi.fn()}
        onOpenWebsite={vi.fn()}
        onDuplicate={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

describe("ProviderCard Grok Official account identity", () => {
  it("renders XaiBoundAccountLine and XaiOauthQuotaFooter for bound Grok official card", () => {
    const provider = grokOfficialProvider("Grok Official (Account 1)", "xai-1");
    renderCard(provider, {
      status: xaiAuthStatus("pilot@x.ai", "xai-1"),
      isCurrent: true,
    });

    expect(screen.getByText("pilot@x.ai")).toBeInTheDocument();
    expect(screen.getByText("xai-oauth-quota")).toBeInTheDocument();
  });

  it("does not render bound account line or quota footer for unbound Grok official card", () => {
    const provider = grokOfficialProvider("Grok Official");
    renderCard(provider, {
      status: xaiAuthStatus("pilot@x.ai", "xai-1"),
      isCurrent: false,
    });

    expect(screen.queryByText("pilot@x.ai")).toBeNull();
    expect(screen.queryByText("xai-oauth-quota")).toBeNull();
  });
});
