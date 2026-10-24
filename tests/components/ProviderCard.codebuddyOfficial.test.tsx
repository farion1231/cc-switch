import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { ManagedAuthStatus } from "@/lib/api";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const quotaFooter = vi.hoisted(() => vi.fn());

vi.mock("@/components/providers/ProviderCardActions", () => ({
  ProviderCardActions: () => null,
}));
vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
vi.mock("@/components/UsageFooter", () => ({ default: () => null }));
vi.mock("@/components/CodebuddyAccountQuotaFooter", () => ({
  default: (props: { accountId: string }) => {
    quotaFooter(props);
    return <div>codebuddy-quota</div>;
  },
}));
vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));
vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: undefined }),
}));

const card = (accountId: string): Provider => ({
  id: "codebuddy-official-1",
  name: "My CodeBuddy",
  category: "official",
  settingsConfig: {},
  meta: {
    authBinding: {
      source: "managed_account",
      authProvider: "codebuddy_oauth",
      accountId,
    },
  },
});

const status: ManagedAuthStatus = {
  provider: "codebuddy_oauth",
  authenticated: true,
  default_account_id: "1@www.codebuddy.cn",
  accounts: [
    {
      id: "1@www.codebuddy.cn",
      provider: "codebuddy_oauth",
      login: "alice",
      avatar_url: null,
      authenticated_at: 0,
      is_default: true,
      github_domain: "www.codebuddy.cn",
      requires_reauth: false,
    },
  ],
};

function renderCard(provider: Provider, onEdit = vi.fn()) {
  const queryClient = createTestQueryClient();
  queryClient.setQueryData(["managed-auth-status", "codebuddy_oauth"], status);
  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderCard
        provider={provider}
        appId="codebuddy"
        isCurrent={false}
        isInConfig={false}
        presentation={{ chips: [], buttons: [] }}
        onEdit={onEdit}
        onDelete={vi.fn()}
        onConfigureUsage={vi.fn()}
        onOpenWebsite={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

describe("ProviderCard CodeBuddy official sign-in", () => {
  it("shows the bound account, its site and its credits", () => {
    renderCard(card("1@www.codebuddy.cn"));
    expect(screen.getByText("alice · 国内版")).toBeInTheDocument();
    expect(screen.getByText("codebuddy-quota")).toBeInTheDocument();
    expect(quotaFooter).toHaveBeenCalledWith({
      accountId: "1@www.codebuddy.cn",
    });
  });

  it("asks to choose again when the bound account was removed", async () => {
    const onEdit = vi.fn();
    const provider = card("9@www.codebuddy.ai");
    renderCard(provider, onEdit);
    expect(screen.getByText("账号已删除，请重新选择")).toBeInTheDocument();
    expect(screen.queryByText("codebuddy-quota")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "选择账号" }));
    expect(onEdit).toHaveBeenCalledWith(provider);
  });
});
