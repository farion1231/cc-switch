import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const claudeQuotaProps = vi.hoisted(() => vi.fn());

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
  ClaudeBoundAccountLine: () => <div>account-line</div>,
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
});
