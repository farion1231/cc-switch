import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
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
vi.mock("@/components/CopilotCliQuotaFooter", () => ({
  default: () => null,
}));
vi.mock("@/components/CodexOauthQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/XaiOauthQuotaFooter", () => ({ default: () => null }));

vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: undefined }),
}));

const XAI_URL = "https://x.ai/grok";

const xaiProvider = (
  accountId: string | null,
  overrides: Partial<Provider> = {},
): Provider => ({
  id: "xai-grok",
  name: "xAI (Grok)",
  category: "third_party",
  websiteUrl: XAI_URL,
  settingsConfig: { env: {} },
  meta: {
    providerType: "xai_oauth",
    ...(accountId
      ? {
          authBinding: {
            source: "managed_account",
            authProvider: "xai_oauth",
            accountId,
          },
        }
      : {}),
  },
  ...overrides,
});

const account = (
  id: string,
  login: string,
  requiresReauth = false,
): ManagedAuthStatus["accounts"][number] => ({
  id,
  provider: "xai_oauth",
  login,
  avatar_url: null,
  authenticated_at: 0,
  is_default: false,
  github_domain: "x.ai",
  reauth_required: false,
  requires_reauth: requiresReauth,
});

const status = (
  accounts: ManagedAuthStatus["accounts"],
  defaultAccountId: string | null = null,
): ManagedAuthStatus => ({
  provider: "xai_oauth",
  authenticated: true,
  default_account_id: defaultAccountId,
  accounts,
});

function renderCard(provider: Provider, planName?: string) {
  const queryClient = createTestQueryClient();
  if (planName) {
    queryClient.setQueryData(
      [
        "xai_oauth",
        "quota",
        provider.meta?.authBinding?.accountId ?? "default",
      ],
      { success: true, tiers: [], planName },
    );
  }
  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderCard
        provider={provider}
        appId="claude"
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

describe("ProviderCard xAI OAuth account subtitle", () => {
  beforeEach(() => {
    authGetStatus.mockReset();
  });

  it("shows the bound xAI account and plan instead of the website URL", async () => {
    authGetStatus.mockResolvedValue(
      status([account("xai-1", "grok@x.ai")], "xai-1"),
    );

    renderCard(xaiProvider("xai-1"), "SuperGrok");

    expect(await screen.findByText("grok@x.ai")).toBeInTheDocument();
    expect(screen.getByText("· SuperGrok")).toBeInTheDocument();
    expect(screen.queryByText(XAI_URL)).not.toBeInTheDocument();
  });

  it("falls back to the xAI account when the legacy Copilot template has no account", async () => {
    authGetStatus.mockImplementation(async (provider) =>
      provider === "github_copilot"
        ? { accounts: [], default_account_id: null }
        : status([account("xai-1", "grok@x.ai")], "xai-1"),
    );
    const provider = xaiProvider("xai-1");
    provider.meta!.usage_script = {
      enabled: true,
      language: "javascript",
      code: "",
      templateType: "github_copilot",
    };

    renderCard(provider);

    expect(await screen.findByText("grok@x.ai")).toBeInTheDocument();
  });

  it("falls back to the default account when the provider is unbound", async () => {
    authGetStatus.mockResolvedValue(
      status(
        [account("xai-1", "grok@x.ai"), account("xai-2", "work@x.ai")],
        "xai-1",
      ),
    );

    renderCard(xaiProvider(null));

    expect(await screen.findByText("grok@x.ai")).toBeInTheDocument();
    expect(screen.queryByText("work@x.ai")).not.toBeInTheDocument();
  });

  it("keeps manual notes on top of the account", async () => {
    authGetStatus.mockResolvedValue(
      status([account("xai-1", "grok@x.ai")], "xai-1"),
    );

    renderCard(xaiProvider("xai-1", { notes: "Work account" }), "SuperGrok");

    expect(await screen.findByText("Work account")).toBeInTheDocument();
    expect(screen.getByText("· SuperGrok")).toBeInTheDocument();
    expect(screen.queryByText("grok@x.ai")).not.toBeInTheDocument();
  });

  it.each([
    ["unbound", null],
    ["bound", "xai-1"],
  ] as const)(
    "refreshes a %s card when the backend expires the default account",
    async (_, accountId) => {
      authGetStatus.mockResolvedValue(
        status(
          [account("xai-1", "grok@x.ai"), account("xai-2", "work@x.ai")],
          "xai-1",
        ),
      );
      vi.useFakeTimers();
      const view = renderCard(xaiProvider(accountId));
      try {
        await act(async () => {
          await vi.advanceTimersByTimeAsync(1);
        });
        expect(screen.getByText("grok@x.ai")).toBeInTheDocument();

        authGetStatus.mockResolvedValue(
          status(
            [
              account("xai-1", "grok@x.ai", true),
              account("xai-2", "work@x.ai"),
            ],
            "xai-2",
          ),
        );
        await act(async () => {
          await vi.advanceTimersByTimeAsync(15_000);
        });
        if (accountId) {
          expect(screen.getByText("grok@x.ai")).toBeInTheDocument();
          expect(
            screen.getByText("xaiOauth.reauthRequired"),
          ).toBeInTheDocument();
          expect(screen.queryByText("work@x.ai")).not.toBeInTheDocument();
        } else {
          expect(screen.getByText("work@x.ai")).toBeInTheDocument();
          expect(screen.queryByText("grok@x.ai")).not.toBeInTheDocument();
        }
      } finally {
        view.unmount();
        vi.useRealTimers();
      }
    },
  );

  it("avoids repeating the login when the card name already contains it", async () => {
    authGetStatus.mockResolvedValue(
      status([account("xai-1", "grok@x.ai")], "xai-1"),
    );

    renderCard(xaiProvider("xai-1", { name: "xAI (grok@x.ai)" }));

    expect(await screen.findByText("xaiOauth.account")).toBeInTheDocument();
  });

  it("shows a reauth warning when the resolved account requires sign-in", async () => {
    authGetStatus.mockResolvedValue(
      status([account("xai-1", "grok@x.ai", true)], "xai-1"),
    );

    renderCard(xaiProvider("xai-1"));

    expect(await screen.findByText("grok@x.ai")).toBeInTheDocument();
    expect(screen.getByText("xaiOauth.reauthRequired")).toBeInTheDocument();
  });

  it("warns when the bound account no longer exists without falling back to default", async () => {
    authGetStatus.mockResolvedValue(
      status([account("xai-2", "work@x.ai")], "xai-2"),
    );

    renderCard(xaiProvider("deleted-account"));

    expect(
      await screen.findByText("copilot.statusUnavailable"),
    ).toBeInTheDocument();
    expect(screen.queryByText("work@x.ai")).not.toBeInTheDocument();
  });

  it("falls back to the website URL while the account status is loading", () => {
    authGetStatus.mockReturnValue(new Promise(() => {}));

    renderCard(xaiProvider("xai-1"));

    expect(screen.queryByText("grok@x.ai")).not.toBeInTheDocument();
    expect(screen.getByText(XAI_URL)).toBeInTheDocument();
  });

  it("does not query xAI account status for a non-xAI provider", () => {
    renderCard({
      id: "plain",
      name: "Plain Provider",
      category: "custom",
      settingsConfig: {},
    });

    expect(authGetStatus).not.toHaveBeenCalled();
  });
});
