import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import { ProviderCard } from "@/components/providers/ProviderCard";

vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: undefined }),
}));

vi.mock("@/components/UsageFooter", () => ({
  default: () => <div data-testid="usage-footer" />,
}));

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => <div data-testid="provider-icon" />,
}));

function createProvider(overrides: Partial<Provider> = {}): Provider {
  return {
    id: "kimi-code",
    name: "Kimi Code",
    settingsConfig: {},
    category: "custom",
    ...overrides,
  };
}

function renderCard(overrides: Partial<Parameters<typeof ProviderCard>[0]> = {}) {
  const provider = overrides.provider ?? createProvider();

  return render(
    <ProviderCard
      provider={provider}
      isCurrent={false}
      appId="kimi"
      isInConfig={false}
      onSwitch={vi.fn()}
      onEdit={vi.fn()}
      onDelete={vi.fn()}
      onRemoveFromConfig={vi.fn()}
      onConfigureUsage={vi.fn()}
      onOpenWebsite={vi.fn()}
      onDuplicate={vi.fn()}
      isProxyRunning={false}
      {...overrides}
    />,
  );
}

describe("ProviderCard", () => {
  it("highlights Kimi providers already written to live config", () => {
    const { container } = renderCard({ isInConfig: true });

    expect(container.firstElementChild).toHaveClass("border-blue-500/60");
    expect(
      screen.getByRole("button", { name: /移除|provider\.removeFromConfig/ }),
    ).toBeInTheDocument();
  });

  it("shows Kimi providers outside live config as addable", () => {
    const { container } = renderCard({ isInConfig: false });

    expect(container.firstElementChild).not.toHaveClass("border-blue-500/60");
    expect(
      screen.getByRole("button", { name: /添加|provider\.addToConfig/ }),
    ).toBeInTheDocument();
  });
});
