import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { ManagedAuthStatus } from "@/lib/api";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/providers/ProviderActions", () => ({
  ProviderActions: () => null,
}));

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => null,
}));

vi.mock("@/components/UsageFooter", () => ({ default: () => null }));
vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: () => null,
}));
vi.mock("@/components/CopilotQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/CodexOauthQuotaFooter", () => ({
  default: () => null,
}));
vi.mock("@/components/XaiOauthQuotaFooter", () => ({ default: () => null }));

vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: undefined }),
}));

const managedProvider = (
  name: string,
  accountId = "account-long",
): Provider => ({
  id: "managed-official",
  name,
  category: "official",
  settingsConfig: {},
  meta: {
    providerType: "codex_oauth",
    authBinding: {
      source: "managed_account",
      authProvider: "codex_oauth",
      accountId,
    },
  },
});

const authStatus = (login: string): ManagedAuthStatus => ({
  provider: "codex_oauth",
  authenticated: true,
  default_account_id: "account-long",
  accounts: [
    {
      id: "account-long",
      provider: "codex_oauth",
      login,
      avatar_url: null,
      authenticated_at: 0,
      is_default: true,
      github_domain: "",
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
    onEdit?: (provider: Provider) => void;
  } = {},
) {
  const queryClient = createTestQueryClient();
  if (options.status) {
    queryClient.setQueryData(
      ["managed-auth-status", "codex_oauth"],
      options.status,
    );
  }

  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderCard
        provider={provider}
        appId="codex"
        isCurrent={options.isCurrent ?? false}
        isProxyRunning={false}
        onSwitch={vi.fn()}
        onEdit={options.onEdit ?? vi.fn()}
        onDelete={vi.fn()}
        onConfigureUsage={vi.fn()}
        onOpenWebsite={vi.fn()}
        onDuplicate={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

describe("ProviderCard Codex Official account identity", () => {
  it("keeps a custom nickname and safely truncates a long account login", () => {
    const login =
      "a-very-long-personal-account-name-that-must-not-expand-the-card@example.com";
    renderCard(managedProvider("Work account"), {
      status: authStatus(login),
    });

    expect(
      screen.getByRole("heading", { level: 3, name: "Work account" }),
    ).toBeInTheDocument();
    expect(screen.getByTitle(login)).toHaveClass("truncate");
    expect(screen.getByTitle(login)).toHaveTextContent(login);
  });

  it("keeps generated and legacy generated provider names as the title", () => {
    const login = "user@example.com";
    const { rerender } = renderCard(managedProvider(login), {
      status: authStatus(login),
    });

    expect(screen.getByRole("heading", { name: login })).toHaveAttribute(
      "title",
      login,
    );
    expect(screen.getByText("OpenAI 账号")).toBeInTheDocument();

    const queryClient = createTestQueryClient();
    queryClient.setQueryData(
      ["managed-auth-status", "codex_oauth"],
      authStatus(login),
    );
    rerender(
      <QueryClientProvider client={queryClient}>
        <ProviderCard
          provider={managedProvider(`OpenAI Official (${login})`)}
          appId="codex"
          isCurrent={false}
          isProxyRunning={false}
          onSwitch={vi.fn()}
          onEdit={vi.fn()}
          onDelete={vi.fn()}
          onConfigureUsage={vi.fn()}
          onOpenWebsite={vi.fn()}
          onDuplicate={vi.fn()}
        />
      </QueryClientProvider>,
    );

    expect(
      screen.getByRole("heading", {
        name: `OpenAI Official (${login})`,
      }),
    ).toHaveClass("truncate");
  });

  it("keeps the provider name on the native card and explains its login", () => {
    renderCard({
      id: "codex-official",
      name: "OpenAI Official",
      category: "official",
      settingsConfig: {},
    });

    expect(
      screen.getByRole("heading", { name: "OpenAI Official" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("账号会随 Codex CLI 当前登录变化"),
    ).toBeInTheDocument();
  });

  it("shows a manual note instead of generated account guidance", () => {
    renderCard({
      id: "codex-official",
      name: "OpenAI Official",
      notes: "Primary work provider",
      category: "official",
      settingsConfig: {},
    });

    expect(screen.getByText("Primary work provider")).toBeInTheDocument();
    expect(
      screen.queryByText("账号会随 Codex CLI 当前登录变化"),
    ).not.toBeInTheDocument();
  });

  it("makes a legacy unbound card actionable without changing it", async () => {
    const user = userEvent.setup();
    const onEdit = vi.fn();
    const provider: Provider = {
      id: "legacy-unbound",
      name: "Legacy Official",
      category: "official",
      settingsConfig: {},
    };
    renderCard(provider, { isCurrent: true, onEdit });

    expect(screen.getByText("尚未选择账号").parentElement).toHaveClass(
      "text-sm",
    );
    const chooseAccount = screen.getByRole("button", { name: "选择账号" });
    expect(chooseAccount).toHaveClass("text-sm");
    expect(screen.queryByText("使用中")).not.toBeInTheDocument();
    await user.click(chooseAccount);
    expect(onEdit).toHaveBeenCalledWith(provider);
  });
});
