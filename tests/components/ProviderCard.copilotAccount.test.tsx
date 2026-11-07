import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { ManagedAuthStatus } from "@/lib/api";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const authGetStatus = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    authApi: { ...actual.authApi, authGetStatus },
  };
});

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
vi.mock("@/components/XaiOauthQuotaFooter", () => ({ default: () => null }));

vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: undefined }),
}));

const copilotProvider = (accountId: string | null): Provider => ({
  id: "copilot-x",
  name: "Copilot-X",
  category: "custom",
  settingsConfig: {},
  meta: {
    providerType: "github_copilot",
    ...(accountId
      ? {
          authBinding: {
            source: "managed_account",
            authProvider: "github_copilot",
            accountId,
          },
        }
      : {}),
  },
});

const account = (
  id: string,
  login: string,
): ManagedAuthStatus["accounts"][number] => ({
  id,
  provider: "github_copilot",
  login,
  avatar_url: null,
  authenticated_at: 0,
  is_default: false,
  github_domain: "github.com",
  reauth_required: false,
  requires_reauth: false,
});

const status = (
  accounts: ManagedAuthStatus["accounts"],
  defaultAccountId: string | null = null,
): ManagedAuthStatus => ({
  provider: "github_copilot",
  authenticated: true,
  default_account_id: defaultAccountId,
  accounts,
});

function renderCard(provider: Provider) {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderCard
        provider={provider}
        appId="claude-desktop"
        isCurrent={false}
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

describe("ProviderCard Copilot account chip", () => {
  beforeEach(() => {
    authGetStatus.mockReset();
  });

  it("shows the GitHub account bound to the provider", async () => {
    authGetStatus.mockResolvedValue(
      status([account("acc-1", "abc@test.com")], "acc-1"),
    );

    renderCard(copilotProvider("acc-1"));

    expect(await screen.findByText("abc@test.com")).toBeInTheDocument();
  });

  it("falls back to the default account when the provider is unbound", async () => {
    authGetStatus.mockResolvedValue(
      status(
        [account("acc-1", "abc@test.com"), account("acc-2", "abc@test.com-b")],
        "acc-1",
      ),
    );

    renderCard(copilotProvider(null));

    expect(await screen.findByText("abc@test.com · 默认")).toBeInTheDocument();
  });

  it("warns when the bound account no longer exists", async () => {
    authGetStatus.mockResolvedValue(
      status([account("acc-2", "abc@test.com-b")], "acc-2"),
    );

    renderCard(copilotProvider("deleted-account"));

    expect(await screen.findByText("账号不可用")).toBeInTheDocument();
  });

  it("renders no account chip while the status is still loading", () => {
    authGetStatus.mockReturnValue(new Promise(() => {}));

    renderCard(copilotProvider("acc-1"));

    expect(screen.queryByText(/abc@test\.com/)).not.toBeInTheDocument();
  });

  it("renders no account chip for a non-copilot provider", () => {
    renderCard({
      id: "plain",
      name: "Plain Provider",
      category: "custom",
      settingsConfig: {},
    });

    expect(screen.queryByText("账号不可用")).not.toBeInTheDocument();
    expect(authGetStatus).not.toHaveBeenCalled();
  });
});
