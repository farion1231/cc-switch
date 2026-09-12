import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/providers/ProviderActions", () => ({
  ProviderActions: ({ isCurrent }: { isCurrent: boolean }) => (
    <span>{isCurrent ? "legacy-current" : "legacy-inactive"}</span>
  ),
}));
vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
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

const provider: Provider = {
  id: "provider-a",
  name: "Provider A",
  category: "third_party",
  settingsConfig: {
    env: {
      ANTHROPIC_AUTH_TOKEN: "secret-not-for-the-badge",
      ANTHROPIC_BASE_URL: "https://api.example.com",
    },
  },
  meta: {
    claudeRouter: {
      enabled: true,
      models: [
        { alias: "sonnet", displayName: "Sonnet", upstreamModel: "model-a" },
        { alias: "opus", displayName: "Opus", upstreamModel: "model-b" },
      ],
    },
  },
};

function renderCard(isCurrent: boolean) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderCard
        provider={provider}
        isCurrent={isCurrent}
        appId="claude"
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
}

describe("ProviderCard Claude router badge", () => {
  it("renders router status independently from legacy active state", () => {
    const view = renderCard(false);
    expect(screen.getByText("Router enabled · 2 models")).toBeInTheDocument();
    expect(screen.getByText("legacy-inactive")).toBeInTheDocument();

    view.unmount();
    renderCard(true);
    expect(screen.getByText("Router enabled · 2 models")).toBeInTheDocument();
    expect(screen.getByText("legacy-current")).toBeInTheDocument();
    expect(
      screen.queryByText("secret-not-for-the-badge"),
    ).not.toBeInTheDocument();
  });
});
