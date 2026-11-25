import { QueryClientProvider } from "@tanstack/react-query";
import { render, renderHook, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import { APP_IDS } from "@/config/appSpecs";
import type { AppId, ManagedAuthProvider, ManagedAuthStatus } from "@/lib/api";
import {
  managedAuthStatusQueryKey,
  useManagedAuthStatus,
} from "@/lib/query/managedAuth";
import { usageKeys } from "@/lib/query/usage";
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
vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
vi.mock("@/components/UsageFooter", () => ({
  default: ({ inline }: { inline: boolean }) =>
    inline ? null : <div>expanded-script-usage</div>,
}));
vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: () => null,
}));
vi.mock("@/components/CopilotQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/CopilotCliQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/CodexOauthQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/XaiOauthQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/AgyAccountQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/ClaudeAccountQuotaFooter", () => ({
  default: () => null,
}));
vi.mock("@/components/KimiAccountQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/CodebuddyAccountQuotaFooter", () => ({
  default: () => null,
}));
vi.mock("@/components/providers/WorkBuddyQuotaFooter", () => ({
  WorkBuddyQuotaFooter: () => null,
}));
vi.mock("@/lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/platform")>()),
  isMac: () => true,
  isLinux: () => false,
  isWindows: () => false,
}));
vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));

const canaryLogin = (provider: ManagedAuthProvider) =>
  `canary-${provider}@leak.test`;

/**
 * Exhaustive poisoned cache for every ManagedAuthProvider.
 * Typing this as `Record<ManagedAuthProvider, ManagedAuthStatus>` guarantees
 * TypeScript fails the build until any newly added auth provider is covered here.
 */
const POISONED_MANAGED_AUTH: Record<ManagedAuthProvider, ManagedAuthStatus> = {
  github_copilot: {
    provider: "github_copilot",
    authenticated: true,
    default_account_id: "canary-github_copilot",
    accounts: [
      {
        id: "canary-github_copilot",
        provider: "github_copilot",
        login: canaryLogin("github_copilot"),
        avatar_url: null,
        authenticated_at: 1,
        is_default: true,
        github_domain: "github.com",
        requires_reauth: true,
      },
    ],
  },
  codex_oauth: {
    provider: "codex_oauth",
    authenticated: true,
    default_account_id: "canary-codex_oauth",
    accounts: [
      {
        id: "canary-codex_oauth",
        provider: "codex_oauth",
        login: canaryLogin("codex_oauth"),
        avatar_url: null,
        authenticated_at: 1,
        is_default: true,
        github_domain: "",
        reauth_required: true,
        requires_reauth: true,
      },
    ],
  },
  xai_oauth: {
    provider: "xai_oauth",
    authenticated: true,
    default_account_id: "canary-xai_oauth",
    accounts: [
      {
        id: "canary-xai_oauth",
        provider: "xai_oauth",
        login: canaryLogin("xai_oauth"),
        avatar_url: null,
        authenticated_at: 1,
        is_default: true,
        github_domain: "x.ai",
        requires_reauth: true,
      },
    ],
  },
  agy_oauth: {
    provider: "agy_oauth",
    authenticated: true,
    default_account_id: "canary-agy_oauth",
    accounts: [
      {
        id: "canary-agy_oauth",
        provider: "agy_oauth",
        login: canaryLogin("agy_oauth"),
        avatar_url: null,
        authenticated_at: 1,
        is_default: true,
        github_domain: "",
        requires_reauth: true,
      },
    ],
  },
  workbuddy_account: {
    provider: "workbuddy_account",
    authenticated: true,
    default_account_id: "canary-workbuddy_account",
    accounts: [
      {
        id: "canary-workbuddy_account",
        provider: "workbuddy_account",
        login: canaryLogin("workbuddy_account"),
        avatar_url: null,
        authenticated_at: 1,
        is_default: true,
        github_domain: "",
        requires_reauth: true,
      },
    ],
  },
  codebuddy_oauth: {
    provider: "codebuddy_oauth",
    authenticated: true,
    default_account_id: "canary-codebuddy_oauth",
    accounts: [
      {
        id: "canary-codebuddy_oauth",
        provider: "codebuddy_oauth",
        login: canaryLogin("codebuddy_oauth"),
        avatar_url: null,
        authenticated_at: 1,
        is_default: true,
        github_domain: "www.codebuddy.cn",
        requires_reauth: true,
      },
    ],
  },
  claude_official: {
    provider: "claude_official",
    authenticated: true,
    default_account_id: "canary-claude_official",
    accounts: [
      {
        id: "canary-claude_official",
        provider: "claude_official",
        login: canaryLogin("claude_official"),
        avatar_url: null,
        authenticated_at: 1,
        is_default: true,
        github_domain: "",
        requires_reauth: true,
      },
    ],
  },
};

const ALL_MANAGED_PROVIDERS = Object.keys(
  POISONED_MANAGED_AUTH,
) as ManagedAuthProvider[];

function createPoisonedQueryClient(providerId: string, appId: AppId) {
  const queryClient = createTestQueryClient();
  for (const authProvider of ALL_MANAGED_PROVIDERS) {
    queryClient.setQueryData(
      managedAuthStatusQueryKey(authProvider),
      POISONED_MANAGED_AUTH[authProvider],
    );
  }
  queryClient.setQueryData(usageKeys.script(providerId, appId), {
    success: true,
    data: [
      { planName: "tier-a", total: 100, used: 10, remaining: 90, unit: "%" },
      { planName: "tier-b", total: 100, used: 20, remaining: 80, unit: "%" },
    ],
  });
  return queryClient;
}

function makeThirdPartyProvider(appId: AppId): Provider {
  return {
    id: `third-party-${appId}`,
    name: `Custom ${appId}`,
    category: "third_party",
    websiteUrl: `https://api.${appId}.example.com`,
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: `https://api.${appId}.example.com`,
        GEMINI_API_KEY: "sk-test",
        GOOGLE_GEMINI_BASE_URL: `https://api.${appId}.example.com`,
      },
      auth: { OPENAI_API_KEY: "sk-test" },
      config: `base_url = "https://api.${appId}.example.com"`,
    },
  };
}

describe("useManagedAuthStatus disabled-observer isolation", () => {
  it("masks cached data, isSuccess, and isError while disabled", () => {
    authGetStatus.mockReset();
    const queryClient = createTestQueryClient();
    queryClient.setQueryData(
      managedAuthStatusQueryKey("agy_oauth"),
      POISONED_MANAGED_AUTH.agy_oauth,
    );

    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    const { result, rerender } = renderHook(
      ({ enabled }: { enabled: boolean }) =>
        useManagedAuthStatus("agy_oauth", { enabled }),
      { initialProps: { enabled: false }, wrapper },
    );

    expect(result.current.data).toBeUndefined();
    expect(result.current.isSuccess).toBe(false);
    expect(result.current.isError).toBe(false);
    expect(authGetStatus).not.toHaveBeenCalled();

    rerender({ enabled: true });
    expect(result.current.data).toEqual(POISONED_MANAGED_AUTH.agy_oauth);
    expect(result.current.isSuccess).toBe(true);
  });
});

describe("ProviderCard poisoned-cache cross-app isolation", () => {
  it.each(APP_IDS)(
    "leaks zero cached managed-auth data onto third-party %s cards",
    (appId) => {
      authGetStatus.mockReset();
      const provider = makeThirdPartyProvider(appId);
      const queryClient = createPoisonedQueryClient(provider.id, appId);

      const { container } = render(
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

      const text = container.textContent ?? "";
      for (const authProvider of ALL_MANAGED_PROVIDERS) {
        expect(text).not.toContain(canaryLogin(authProvider));
      }
      expect(screen.queryByText("authCenter.inUseBy")).not.toBeInTheDocument();
      expect(
        screen.queryByText("authCenter.needsReauth"),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByText("expanded-script-usage"),
      ).not.toBeInTheDocument();
      expect(authGetStatus).not.toHaveBeenCalled();
    },
  );

  it.each<AppId>(["claude", "codex", "gemini", "copilot", "grokbuild", "kimi"])(
    "leaks zero foreign cached auth data or script tiers onto official %s cards",
    (appId) => {
      const provider: Provider = {
        id: `official-${appId}`,
        name: `Official ${appId}`,
        category: "official",
        settingsConfig: { env: {}, auth: {}, config: "" },
      };
      const queryClient = createPoisonedQueryClient(provider.id, appId);

      const { container } = render(
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

      const text = container.textContent ?? "";
      for (const authProvider of ALL_MANAGED_PROVIDERS) {
        expect(text).not.toContain(canaryLogin(authProvider));
      }
      expect(screen.queryByText("authCenter.inUseBy")).not.toBeInTheDocument();
      expect(
        screen.queryByText("expanded-script-usage"),
      ).not.toBeInTheDocument();
    },
  );

  it("limits Antigravity official cards strictly to agy_oauth cache", () => {
    const provider: Provider = {
      id: "official-agy",
      name: "Antigravity",
      category: "official",
      settingsConfig: { config: {} },
    };
    const queryClient = createPoisonedQueryClient(provider.id, "agy");

    const { container } = render(
      <QueryClientProvider client={queryClient}>
        <ProviderCard
          provider={provider}
          appId="agy"
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

    const text = container.textContent ?? "";
    expect(text).toContain(canaryLogin("agy_oauth"));
    expect(screen.getByText("authCenter.inUseBy")).toBeInTheDocument();
    for (const authProvider of ALL_MANAGED_PROVIDERS) {
      if (authProvider === "agy_oauth") continue;
      expect(text).not.toContain(canaryLogin(authProvider));
    }
    expect(screen.queryByText("expanded-script-usage")).not.toBeInTheDocument();
  });
});
