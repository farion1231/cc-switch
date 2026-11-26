import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { ManagedAuthStatus } from "@/lib/api";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const authGetStatus = vi.hoisted(() => vi.fn());
const xaiQuotaFooterProps = vi.hoisted(() => vi.fn());
const subscriptionQuotaFooterProps = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    authApi: { ...actual.authApi, authGetStatus },
  };
});

vi.mock("@/components/providers/ProviderCardActions", () => ({
  ProviderCardActions: (props: { onConfigureUsage?: () => void }) =>
    props.onConfigureUsage ? (
      <button onClick={props.onConfigureUsage}>configure-usage</button>
    ) : null,
}));

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => null,
}));

vi.mock("@/components/UsageFooter", () => ({ default: () => null }));
vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: (props: unknown) => {
    subscriptionQuotaFooterProps(props);
    return <div>grok-cli-quota</div>;
  },
}));
vi.mock("@/components/CopilotQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/CopilotCliQuotaFooter", () => ({
  default: () => null,
}));
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

const GROK_URL = "https://x.ai/grok";

const boundGrokOfficial = (
  accountId = "xai-1",
  overrides: Partial<Provider> = {},
): Provider => ({
  id: "grokbuild-official",
  name: "Grok Official",
  category: "official",
  websiteUrl: GROK_URL,
  settingsConfig: { config: "" },
  meta: {
    authBinding: {
      source: "managed_account",
      authProvider: "xai_oauth",
      accountId,
    },
  },
  ...overrides,
});

const unboundGrokOfficial = (
  usageEnabled: boolean,
  overrides: Partial<Provider> = {},
): Provider => ({
  id: "grokbuild-official",
  name: "Grok Official",
  category: "official",
  websiteUrl: GROK_URL,
  settingsConfig: { config: "" },
  meta: {
    usage_script: {
      enabled: usageEnabled,
      language: "javascript",
      code: "",
      templateType: "official_subscription",
    },
  },
  ...overrides,
});

const xaiStatus = (
  accounts: ManagedAuthStatus["accounts"],
  defaultAccountId: string | null = accounts[0]?.id ?? null,
): ManagedAuthStatus => ({
  provider: "xai_oauth",
  authenticated: accounts.length > 0,
  default_account_id: defaultAccountId,
  accounts,
});

const xaiAccount = (
  id: string,
  login: string,
): ManagedAuthStatus["accounts"][number] => ({
  id,
  provider: "xai_oauth",
  login,
  avatar_url: null,
  authenticated_at: 0,
  is_default: false,
  github_domain: "x.ai",
  reauth_required: false,
  requires_reauth: false,
});

function renderGrokCard(
  provider: Provider,
  options: {
    isCurrent?: boolean;
    planName?: string;
    onConfigureUsage?: (provider: Provider) => void;
  } = {},
) {
  const queryClient = createTestQueryClient();
  const boundId = provider.meta?.authBinding?.accountId;
  if (options.planName) {
    queryClient.setQueryData(
      boundId
        ? ["xai_oauth", "quota", boundId]
        : ["subscription", "quota", "grokbuild"],
      { success: true, tiers: [], planName: options.planName },
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
        onConfigureUsage={options.onConfigureUsage ?? vi.fn()}
        onOpenWebsite={vi.fn()}
        onDuplicate={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

describe("ProviderCard Grok Build quota and subscription tier", () => {
  beforeEach(() => {
    authGetStatus.mockReset();
    xaiQuotaFooterProps.mockClear();
    subscriptionQuotaFooterProps.mockClear();
  });

  it("shows the bound xAI account, subscription tier, quota, and usage config", async () => {
    const user = userEvent.setup();
    const onConfigureUsage = vi.fn();
    authGetStatus.mockResolvedValue(
      xaiStatus([xaiAccount("xai-1", "grok@x.ai")]),
    );
    const provider = boundGrokOfficial("xai-1");

    renderGrokCard(provider, {
      isCurrent: true,
      planName: "SuperGrok Heavy",
      onConfigureUsage,
    });

    expect(await screen.findByText("grok@x.ai")).toBeInTheDocument();
    expect(screen.getByText("· SuperGrok Heavy")).toBeInTheDocument();
    expect(screen.getByText("xai-oauth-quota")).toBeInTheDocument();
    expect(xaiQuotaFooterProps).toHaveBeenCalledWith(
      expect.objectContaining({ autoQueryInterval: 5, isCurrent: true }),
    );
    await user.click(screen.getByRole("button", { name: "configure-usage" }));
    expect(onConfigureUsage).toHaveBeenCalledWith(provider);
  });

  it("passes the saved polling interval to bound Grok Official quota", () => {
    authGetStatus.mockResolvedValue(
      xaiStatus([xaiAccount("xai-1", "grok@x.ai")]),
    );
    const provider = boundGrokOfficial("xai-1");
    provider.meta!.usage_script = {
      enabled: true,
      language: "javascript",
      code: "",
      templateType: "official_subscription",
      autoQueryInterval: 15,
    };

    renderGrokCard(provider, { isCurrent: true });

    expect(xaiQuotaFooterProps).toHaveBeenCalledWith(
      expect.objectContaining({ autoQueryInterval: 15 }),
    );
  });

  it("hides quota and subscription tier when bound Grok usage is disabled", async () => {
    authGetStatus.mockResolvedValue(
      xaiStatus([xaiAccount("xai-1", "grok@x.ai")]),
    );
    const provider = boundGrokOfficial("xai-1");
    provider.meta!.usage_script = {
      enabled: false,
      language: "javascript",
      code: "",
      templateType: "official_subscription",
    };

    renderGrokCard(provider, { isCurrent: true, planName: "SuperGrok" });

    expect(await screen.findByText("grok@x.ai")).toBeInTheDocument();
    expect(screen.queryByText("· SuperGrok")).not.toBeInTheDocument();
    expect(screen.queryByText("xai-oauth-quota")).not.toBeInTheDocument();
    expect(xaiQuotaFooterProps).not.toHaveBeenCalled();
  });

  it("refreshes bound Grok login status while quota is disabled", async () => {
    const account = xaiAccount("xai-1", "grok@x.ai");
    authGetStatus.mockResolvedValue(xaiStatus([account]));
    const provider = boundGrokOfficial();
    provider.meta!.usage_script = {
      enabled: false,
      language: "javascript",
      code: "",
    };
    vi.useFakeTimers();
    const view = renderGrokCard(provider);
    try {
      await act(() => vi.advanceTimersByTimeAsync(1));
      expect(screen.getByText("grok@x.ai")).toBeInTheDocument();

      authGetStatus.mockResolvedValue(
        xaiStatus([{ ...account, requires_reauth: true }]),
      );
      await act(() => vi.advanceTimersByTimeAsync(15_000));
      expect(screen.getByText("需要重新登录")).toBeInTheDocument();
    } finally {
      view.unmount();
      vi.useRealTimers();
    }
  });

  it("keeps manual notes alongside the subscription tier on bound cards", async () => {
    authGetStatus.mockResolvedValue(
      xaiStatus([xaiAccount("xai-1", "grok@x.ai")]),
    );

    renderGrokCard(boundGrokOfficial("xai-1", { notes: "Team Grok" }), {
      planName: "SuperGrok",
    });

    expect(await screen.findByText("Team Grok")).toBeInTheDocument();
    expect(screen.getByText("· SuperGrok")).toBeInTheDocument();
    expect(screen.queryByText("grok@x.ai")).not.toBeInTheDocument();
  });

  it("shows quota and subscription tier on unbound Grok Official when enabled", () => {
    renderGrokCard(unboundGrokOfficial(true), {
      isCurrent: true,
      planName: "SuperGrok",
    });

    expect(screen.getByText(GROK_URL)).toBeInTheDocument();
    expect(screen.getByText("· SuperGrok")).toBeInTheDocument();
    expect(screen.getByText("grok-cli-quota")).toBeInTheDocument();
    expect(subscriptionQuotaFooterProps).toHaveBeenCalledWith(
      expect.objectContaining({ appId: "grokbuild", isCurrent: true }),
    );
    expect(authGetStatus).not.toHaveBeenCalled();
  });

  it("hides quota and subscription tier on unbound Grok Official when disabled", () => {
    renderGrokCard(unboundGrokOfficial(false), {
      isCurrent: true,
      planName: "SuperGrok",
    });

    expect(screen.getByText(GROK_URL)).toBeInTheDocument();
    expect(screen.queryByText("· SuperGrok")).not.toBeInTheDocument();
    expect(screen.queryByText("grok-cli-quota")).not.toBeInTheDocument();
    expect(subscriptionQuotaFooterProps).not.toHaveBeenCalled();
  });
});
