import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));

function renderCard(name: string) {
  const provider: Provider = {
    id: `id-${name}`,
    name,
    category: "custom",
    settingsConfig: { env: { ANTHROPIC_BASE_URL: "https://example.com" } },
  };
  return (
    <ProviderCard
      key={provider.id}
      provider={provider}
      appId="claude"
      isCurrent={false}
      presentation={{
        chips: [],
        buttons: [{ key: "enable", label: "Enable", onClick: vi.fn() }],
      }}
      onEdit={vi.fn()}
      onDelete={vi.fn()}
      onConfigureUsage={vi.fn()}
      onOpenWebsite={vi.fn()}
      onDuplicate={vi.fn()}
    />
  );
}

describe("ProviderCard accessible name", () => {
  it("is a group named after its provider, so each card's actions can be told apart", () => {
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        {renderCard("Alpha")}
        {renderCard("Beta")}
      </QueryClientProvider>,
    );

    const beta = screen.getByRole("group", { name: "Beta" });
    expect(
      within(beta).getByRole("button", { name: "Enable" }),
    ).toBeInTheDocument();
    expect(
      within(beta).getByRole("heading", { name: "Beta" }),
    ).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Enable" })).toHaveLength(2);
  });
});
