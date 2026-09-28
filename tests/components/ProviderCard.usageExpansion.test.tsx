import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { TemplateType } from "@/config/constants";
import { usageKeys } from "@/lib/query/usage";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/providers/ProviderActions", () => ({
  ProviderActions: () => null,
}));
vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
vi.mock("@/components/UsageFooter", () => ({
  default: ({ inline }: { inline: boolean }) =>
    inline ? null : <div>expanded-plan-details</div>,
}));
vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: () => <div>official-subscription-quota</div>,
}));
vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));

function renderCard({
  official = false,
  templateType = "custom",
  enabled = true,
}: {
  official?: boolean;
  templateType?: TemplateType;
  enabled?: boolean;
} = {}) {
  const provider: Provider = {
    id: "cached-usage-provider",
    name: "Usage provider",
    category: official ? "official" : "custom",
    settingsConfig: {
      env: { ANTHROPIC_BASE_URL: "https://example.com" },
    },
    meta: {
      usage_script: { enabled, language: "javascript", code: "", templateType },
    },
  };
  const queryClient = createTestQueryClient();
  // A disabled React Query observer still receives existing cache entries.
  queryClient.setQueryData(usageKeys.script(provider.id, "claude"), {
    success: true,
    data: [
      { planName: "five_hour", total: 100, used: 0, remaining: 100, unit: "%" },
      { planName: "seven_day", total: 100, used: 30, remaining: 70, unit: "%" },
    ],
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderCard
        provider={provider}
        appId="claude"
        isCurrent={true}
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

describe("ProviderCard cached usage expansion", () => {
  it("keeps official subscription quota inline despite cached script tiers", () => {
    renderCard({ official: true, templateType: "official_subscription" });

    expect(screen.getByText("official-subscription-quota")).toBeInTheDocument();
    expect(screen.queryByText("expanded-plan-details")).not.toBeInTheDocument();
    expect(screen.queryByTitle("收起")).not.toBeInTheDocument();
    expect(screen.queryByTitle("展开")).not.toBeInTheDocument();
  });

  it.each<Parameters<typeof renderCard>[0]>([
    { official: true },
    { templateType: "official_subscription" },
    { templateType: "token_plan" },
    { enabled: false },
  ])("does not expand ineligible cached usage: %j", (options) => {
    renderCard(options);

    expect(screen.queryByText("expanded-plan-details")).not.toBeInTheDocument();
    expect(screen.queryByTitle("收起")).not.toBeInTheDocument();
    expect(screen.queryByTitle("展开")).not.toBeInTheDocument();
  });

  it("still expands ordinary multi-plan usage and allows collapsing it", async () => {
    const user = userEvent.setup();
    renderCard();

    expect(screen.getByText("expanded-plan-details")).toBeInTheDocument();
    await user.click(screen.getByTitle("收起"));
    expect(screen.queryByText("expanded-plan-details")).not.toBeInTheDocument();
    expect(screen.getByTitle("展开")).toBeInTheDocument();
  });
});
