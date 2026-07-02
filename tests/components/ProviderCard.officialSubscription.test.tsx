import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({
    data: {
      success: true,
      data: [{ planName: "five_hour" }, { planName: "seven_day" }],
    },
  }),
}));

vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));

vi.mock("@/components/UsageFooter", () => ({
  default: () => <div data-testid="legacy-package-usage" />,
}));

vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: () => <div data-testid="official-quota-inline" />,
  OfficialSubscriptionDetails: () => (
    <div data-testid="official-quota-details" />
  ),
}));

vi.mock("@/components/providers/ProviderActions", () => ({
  ProviderActions: () => null,
}));

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => null,
}));

import { ProviderCard } from "@/components/providers/ProviderCard";

describe("ProviderCard official subscription details", () => {
  it("ignores stale multi-plan usage data for the official subscription template", async () => {
    const provider: Provider = {
      id: "codex-official",
      name: "OpenAI Official",
      settingsConfig: { auth: {}, config: "" },
      websiteUrl: "https://chatgpt.com/codex",
      category: "official",
      createdAt: 1,
      meta: {
        usage_script: {
          enabled: true,
          language: "javascript",
          code: "",
          templateType: "official_subscription",
          includeResetCredits: true,
        },
      },
    };
    const noop = vi.fn();

    render(
      <ProviderCard
        provider={provider}
        isCurrent
        appId="codex"
        onSwitch={noop}
        onEdit={noop}
        onDelete={noop}
        onConfigureUsage={noop}
        onOpenWebsite={noop}
        onDuplicate={noop}
        isProxyRunning={false}
      />,
    );

    await waitFor(() =>
      expect(screen.getByTestId("official-quota-details")).toBeInTheDocument(),
    );
    expect(screen.queryByTestId("legacy-package-usage")).toBeNull();
  });
});
