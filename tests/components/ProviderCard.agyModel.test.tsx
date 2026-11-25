import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { AppId, ManagedAuthStatus } from "@/lib/api";
import { agyAuthStatusQuery } from "@/lib/query/subscription";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/providers/ProviderCardActions", () => ({
  ProviderCardActions: () => null,
}));
vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
vi.mock("@/components/UsageFooter", () => ({ default: () => null }));
vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: ({ appId }: { appId: string }) => (
    <span data-testid="subscription-quota">{appId}</span>
  ),
}));
vi.mock("@/components/CopilotQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/CodexOauthQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/XaiOauthQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/AgyAccountQuotaFooter", () => ({
  default: ({ accountId }: { accountId: string }) => (
    <span data-testid="agy-account-quota">{accountId}</span>
  ),
}));
const platform = vi.hoisted(() => ({ mac: false, linux: false }));
vi.mock("@/lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/platform")>()),
  isMac: () => platform.mac,
  isLinux: () => platform.linux,
  isWindows: () => false,
}));
vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));
vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: undefined }),
}));

function renderCard(
  provider: Provider,
  appId: AppId = "agy",
  cachedAgyAuthStatus?: ManagedAuthStatus,
) {
  const queryClient = createTestQueryClient();
  if (cachedAgyAuthStatus) {
    // A disabled React Query observer still receives existing cache entries.
    queryClient.setQueryData(agyAuthStatusQuery.queryKey, cachedAgyAuthStatus);
  }
  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderCard
        provider={provider}
        appId={appId}
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

afterEach(() => {
  platform.mac = false;
  platform.linux = false;
});

const relay: Provider = {
  id: "relay",
  name: "Relay",
  settingsConfig: {
    env: {
      GEMINI_API_KEY: "k",
      GOOGLE_GEMINI_BASE_URL: "https://relay.example.com",
    },
    config: { model: { name: "Gemini 3.1 Pro (High)" } },
  },
};

describe("ProviderCard Antigravity model", () => {
  it("shows the configured model instead of the endpoint", () => {
    renderCard(relay);
    expect(screen.getByText("Gemini 3.1 Pro (High)")).toBeInTheDocument();
    expect(
      screen.queryByText("https://relay.example.com"),
    ).not.toBeInTheDocument();
  });

  it("falls back to GEMINI_MODEL and leaves other apps alone", () => {
    renderCard({
      ...relay,
      settingsConfig: { env: { GEMINI_MODEL: "Gemini 3.8 Flash (Low)" } },
    });
    expect(screen.getByText("Gemini 3.8 Flash (Low)")).toBeInTheDocument();
  });

  it("keeps showing the endpoint for Gemini CLI cards", () => {
    renderCard(relay, "gemini");
    expect(screen.queryByText("Gemini 3.1 Pro (High)")).not.toBeInTheDocument();
  });
});

describe("ProviderCard Antigravity official quota", () => {
  const official: Provider = {
    id: "official",
    name: "Antigravity",
    category: "official",
    settingsConfig: { config: {} },
    meta: {
      usage_script: {
        enabled: true,
        language: "javascript",
        code: "",
        templateType: "official_subscription",
      },
    },
  };

  it("shows the signed-in quota once official usage is turned on", () => {
    renderCard(official);
    expect(screen.getByTestId("subscription-quota")).toHaveTextContent("agy");
  });

  it("stays hidden while usage is off", () => {
    renderCard({
      ...official,
      meta: {
        usage_script: { ...official.meta!.usage_script!, enabled: false },
      },
    });
    expect(screen.queryByTestId("subscription-quota")).not.toBeInTheDocument();
  });

  const bound: Provider = {
    ...official,
    meta: {
      ...official.meta,
      authBinding: {
        source: "managed_account",
        authProvider: "agy_oauth",
        accountId: "sub-b",
      },
    },
  };

  it.each(["mac", "linux"] as const)(
    "shows the bound account's quota on %s",
    (os) => {
      platform.mac = os === "mac";
      platform.linux = os === "linux";
      renderCard(bound);
      expect(screen.getByTestId("agy-account-quota")).toHaveTextContent(
        "sub-b",
      );
      expect(
        screen.queryByTestId("subscription-quota"),
      ).not.toBeInTheDocument();
    },
  );

  it("ignores the binding where accounts cannot be switched", () => {
    platform.mac = false;
    platform.linux = false;
    renderCard(bound);
    expect(screen.getByTestId("subscription-quota")).toHaveTextContent("agy");
    expect(screen.queryByTestId("agy-account-quota")).not.toBeInTheDocument();
  });
});

describe("ProviderCard cached Antigravity auth status isolation", () => {
  const cachedAgyAuthStatus: ManagedAuthStatus = {
    provider: "agy_oauth",
    authenticated: true,
    default_account_id: "agy-active",
    accounts: [
      {
        id: "agy-active",
        provider: "agy_oauth",
        login: "alice@example.com",
        avatar_url: null,
        authenticated_at: 1,
        is_default: true,
        github_domain: "",
        requires_reauth: false,
      },
      {
        id: "agy-other",
        provider: "agy_oauth",
        login: "bob@example.com",
        avatar_url: null,
        authenticated_at: 2,
        is_default: false,
        github_domain: "",
        requires_reauth: false,
      },
    ],
  };

  const agyOfficial: Provider = {
    id: "agy-official",
    name: "Antigravity",
    category: "official",
    settingsConfig: { config: {} },
  };

  it("does not leak cached agy badge or email onto non-agy third-party cards", () => {
    platform.mac = true;
    const codexThirdParty: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      websiteUrl: "https://api.deepseek.com",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-test" },
        config: 'base_url = "https://api.deepseek.com"',
      },
    };

    renderCard(codexThirdParty, "codex", cachedAgyAuthStatus);

    expect(screen.getByText("https://api.deepseek.com")).toBeInTheDocument();
    expect(screen.queryByText("alice@example.com")).not.toBeInTheDocument();
    expect(screen.queryByText("authCenter.inUseBy")).not.toBeInTheDocument();
  });

  it("does not leak cached agy badge onto non-agy official cards", () => {
    platform.mac = true;
    const codexOfficial: Provider = {
      id: "codex-official",
      name: "OpenAI Official",
      category: "official",
      settingsConfig: { auth: {}, config: "" },
    };

    renderCard(codexOfficial, "codex", cachedAgyAuthStatus);

    expect(screen.queryByText("alice@example.com")).not.toBeInTheDocument();
    expect(screen.queryByText("authCenter.inUseBy")).not.toBeInTheDocument();
  });

  it("does not leak cached agy badge or email onto non-official agy cards", () => {
    platform.mac = true;
    renderCard(relay, "agy", cachedAgyAuthStatus);

    expect(screen.getByText("Gemini 3.1 Pro (High)")).toBeInTheDocument();
    expect(screen.queryByText("alice@example.com")).not.toBeInTheDocument();
    expect(screen.queryByText("authCenter.inUseBy")).not.toBeInTheDocument();
  });

  it("does not show cached agy badge or email where account switching is unsupported", () => {
    platform.mac = false;
    platform.linux = false;
    renderCard(agyOfficial, "agy", cachedAgyAuthStatus);

    expect(screen.queryByText("alice@example.com")).not.toBeInTheDocument();
    expect(screen.queryByText("authCenter.inUseBy")).not.toBeInTheDocument();
  });

  it("shows active agy email and in-use badge on supported Antigravity official cards", () => {
    platform.mac = true;
    renderCard(agyOfficial, "agy", cachedAgyAuthStatus);

    expect(screen.getByText("alice@example.com")).toBeInTheDocument();
    expect(screen.getByText("authCenter.inUseBy")).toBeInTheDocument();
  });

  it("shows bound account email without in-use badge when another agy account is active", () => {
    platform.mac = true;
    renderCard(
      {
        ...agyOfficial,
        meta: {
          authBinding: {
            source: "managed_account",
            authProvider: "agy_oauth",
            accountId: "agy-other",
          },
        },
      },
      "agy",
      cachedAgyAuthStatus,
    );

    expect(screen.getByText("bob@example.com")).toBeInTheDocument();
    expect(screen.queryByText("alice@example.com")).not.toBeInTheDocument();
    expect(screen.queryByText("authCenter.inUseBy")).not.toBeInTheDocument();
  });
});
