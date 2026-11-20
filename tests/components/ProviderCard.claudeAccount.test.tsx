import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const claudeQuotaProps = vi.hoisted(() => vi.fn());
const getClaudeDefaultPlan = vi.hoisted(() => vi.fn());
const claudeLineProps = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api/subscription", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/lib/api/subscription")>();
  return {
    subscriptionApi: { ...actual.subscriptionApi, getClaudeDefaultPlan },
  };
});

vi.mock("@/components/providers/ProviderCardActions", () => ({
  ProviderCardActions: () => null,
}));
vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
vi.mock("@/components/UsageFooter", () => ({ default: () => null }));
vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: () => <div>default-login-quota</div>,
}));
vi.mock("@/components/ClaudeAccountQuotaFooter", () => ({
  default: (props: unknown) => {
    claudeQuotaProps(props);
    return <div>claude-account-quota</div>;
  },
}));
vi.mock("@/components/providers/forms/ClaudeOfficialSection", () => ({
  ClaudeBoundAccountLine: (props: { showPlan?: boolean }) => {
    claudeLineProps(props);
    return <div>account-line</div>;
  },
}));
vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));
vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: undefined }),
}));

const boundCard = (meta: Provider["meta"] = {}): Provider => ({
  id: "claude-work",
  name: "Claude Work",
  category: "official",
  settingsConfig: { env: {} },
  meta: {
    authBinding: {
      source: "managed_account",
      authProvider: "claude_official",
      accountId: "work:org",
    },
    ...meta,
  },
});

const officialUsage = (enabled: boolean): Provider["meta"] => ({
  usage_script: {
    enabled,
    language: "javascript",
    code: "",
    templateType: "official_subscription",
  },
});

function renderCard(provider: Provider, isCurrent = false) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderCard
        provider={provider}
        appId="claude"
        isCurrent={isCurrent}
        presentation={{ chips: [], buttons: [] }}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onConfigureUsage={vi.fn()}
        onOpenWebsite={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

describe("ProviderCard with a Claude official account", () => {
  it("shows the bound account's quota by default, even when not current", () => {
    claudeQuotaProps.mockClear();
    renderCard(boundCard());
    expect(screen.getByText("account-line")).toBeInTheDocument();
    expect(screen.getByText("claude-account-quota")).toBeInTheDocument();
    expect(screen.queryByText("default-login-quota")).not.toBeInTheDocument();
    expect(claudeQuotaProps).toHaveBeenCalledWith(
      expect.objectContaining({ accountId: "work:org", isCurrent: false }),
    );
  });

  it("hides the quota when the user turned usage off", () => {
    renderCard(
      boundCard({
        usage_script: {
          enabled: false,
          language: "javascript",
          code: "",
          templateType: "official_subscription",
        },
      }),
      true,
    );
    expect(screen.queryByText("claude-account-quota")).not.toBeInTheDocument();
  });

  it("shows the default login's plan on a card without an account", async () => {
    getClaudeDefaultPlan.mockReset().mockResolvedValue({
      subscriptionType: "max",
      rateLimitTier: "default_claude_max_20x",
    });
    renderCard({
      id: "claude-official",
      name: "Claude Official",
      category: "official",
      websiteUrl: "https://www.anthropic.com/claude-code",
      settingsConfig: { env: {} },
      meta: officialUsage(true),
    });
    expect(await screen.findByText("· Max 20x")).toBeInTheDocument();
    expect(
      screen.getByText("https://www.anthropic.com/claude-code"),
    ).toBeInTheDocument();
  });

  it("doesn't read the default login's plan when usage is off or an account is bound", () => {
    getClaudeDefaultPlan.mockReset();
    renderCard(boundCard());
    renderCard({
      id: "claude-official",
      name: "Claude Official",
      category: "official",
      websiteUrl: "https://www.anthropic.com/claude-code",
      settingsConfig: { env: {} },
      meta: officialUsage(false),
    });
    expect(getClaudeDefaultPlan).not.toHaveBeenCalled();
  });

  it("only shows the bound account's plan when usage is on", () => {
    claudeLineProps.mockClear();
    renderCard(boundCard());
    renderCard(boundCard(officialUsage(false)));
    expect(claudeLineProps.mock.calls.map(([props]) => props.showPlan)).toEqual(
      [true, false],
    );
  });
});
