import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { ManagedAuthStatus } from "@/lib/api";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const codexQuotaFooterProps = vi.hoisted(() => vi.fn());

vi.mock("@/components/providers/ProviderActions", () => ({
  ProviderActions: (props: {
    onDuplicate?: () => void;
    onConfigureUsage?: () => void;
  }) => (
    <>
      {props.onDuplicate ? (
        <button onClick={props.onDuplicate}>duplicate-provider</button>
      ) : null}
      {props.onConfigureUsage ? (
        <button onClick={props.onConfigureUsage}>configure-usage</button>
      ) : null}
    </>
  ),
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
  default: (props: unknown) => {
    codexQuotaFooterProps(props);
    return <div>codex-oauth-quota</div>;
  },
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
  settingsConfig: { auth: {}, config: "" },
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
    onConfigureUsage?: (provider: Provider) => void;
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
        onConfigureUsage={options.onConfigureUsage ?? vi.fn()}
        onOpenWebsite={vi.fn()}
        onDuplicate={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

describe("ProviderCard Codex Official account identity", () => {
  it("keeps existing managed OAuth quota enabled and exposes its configuration", async () => {
    const user = userEvent.setup();
    const onConfigureUsage = vi.fn();
    const provider = managedProvider("Work account");
    delete provider.meta!.providerType;

    renderCard(provider, { isCurrent: true, onConfigureUsage });

    expect(codexQuotaFooterProps).toHaveBeenCalledWith(
      expect.objectContaining({ autoQueryInterval: 5 }),
    );
    await user.click(screen.getByRole("button", { name: "configure-usage" }));
    expect(onConfigureUsage).toHaveBeenCalledWith(provider);
  });

  it("honors a saved disabled state for managed OAuth quota", () => {
    const provider = managedProvider("Work account");
    provider.meta!.usage_script = {
      enabled: false,
      language: "javascript",
      code: "",
      templateType: "official_subscription",
      autoQueryInterval: 12,
    };

    renderCard(provider, { isCurrent: true });

    expect(codexQuotaFooterProps).not.toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "configure-usage" }),
    ).toBeInTheDocument();
  });

  it("passes the saved polling interval to managed OAuth quota", () => {
    const provider = managedProvider("Work account");
    provider.meta!.usage_script = {
      enabled: true,
      language: "javascript",
      code: "",
      templateType: "official_subscription",
      autoQueryInterval: 12,
    };

    renderCard(provider, { isCurrent: true });

    expect(codexQuotaFooterProps).toHaveBeenCalledWith(
      expect.objectContaining({ autoQueryInterval: 12 }),
    );
  });

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
      settingsConfig: { auth: {}, config: "" },
    });

    expect(
      screen.getByRole("heading", { name: "OpenAI Official" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("账号会随 Codex CLI 当前登录变化"),
    ).toBeInTheDocument();
    expect(screen.queryByText("codex-oauth-quota")).not.toBeInTheDocument();
  });

  it("shows the managed account when it is bound to the fixed card", () => {
    const login = "fixed@example.com";
    renderCard(
      {
        ...managedProvider("OpenAI Official"),
        id: "codex-official",
      },
      { status: authStatus(login) },
    );

    expect(screen.getByText(login)).toBeInTheDocument();
    expect(
      screen.queryByText("账号会随 Codex CLI 当前登录变化"),
    ).not.toBeInTheDocument();
    expect(screen.getByText("codex-oauth-quota")).toBeInTheDocument();
  });

  it("shows a manual note instead of generated account guidance", () => {
    renderCard({
      id: "codex-official",
      name: "OpenAI Official",
      notes: "Primary work provider",
      category: "official",
      settingsConfig: { auth: {}, config: "" },
    });

    expect(screen.getByText("Primary work provider")).toBeInTheDocument();
    expect(
      screen.queryByText("账号会随 Codex CLI 当前登录变化"),
    ).not.toBeInTheDocument();
  });

  it("treats a legacy unbound Official card as follow-login", () => {
    const provider: Provider = {
      id: "legacy-unbound",
      name: "Legacy Official",
      category: "official",
      settingsConfig: { auth: {}, config: "" },
      meta: { providerType: "codex_oauth" },
    };
    renderCard(provider, { isCurrent: true });

expect(
      screen.getByText("账号会随 Codex CLI 当前登录变化"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "选择账号" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "duplicate-provider" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("codex-oauth-quota")).not.toBeInTheDocument();
    expect(codexQuotaFooterProps).not.toHaveBeenCalled();
  });

  it("does not label a stored API-key card as follow-login", () => {
    renderCard({
      id: "legacy-api-key",
      name: "Legacy API Key",
      category: "official",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-stored" },
        config: "",
      },
    });

    expect(
      screen.queryByText("账号会随 Codex CLI 当前登录变化"),
    ).not.toBeInTheDocument();
  });
});
