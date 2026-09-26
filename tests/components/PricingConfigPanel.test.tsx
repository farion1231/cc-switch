import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  translate,
  useModelPricing,
  useDeleteModelPricing,
  getDefaultCostMultiplier,
  getPricingModelSource,
} = vi.hoisted(() => ({
  translate: vi.fn((key: string) => key),
  useModelPricing: vi.fn(),
  useDeleteModelPricing: vi.fn(),
  getDefaultCostMultiplier: vi.fn(),
  getPricingModelSource: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: translate }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

vi.mock("@/lib/query/usage", () => ({
  useModelPricing,
  useDeleteModelPricing,
}));

vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getDefaultCostMultiplier,
    getPricingModelSource,
  },
}));

vi.mock("@/components/usage/ModelsDevAutoSyncPanel", () => ({
  ModelsDevAutoSyncPanel: () => null,
}));

import { PricingConfigPanel } from "@/components/usage/PricingConfigPanel";

const pricing = [
  {
    modelId: "vendor/model-alpha",
    displayName: "Claude Alpha",
    inputCostPerMillion: "1",
    outputCostPerMillion: "2",
    cacheReadCostPerMillion: "0.1",
    cacheCreationCostPerMillion: "0.2",
  },
  {
    modelId: "gpt-5.4",
    displayName: "OpenAI flagship",
    inputCostPerMillion: "3",
    outputCostPerMillion: "4",
    cacheReadCostPerMillion: "0.3",
    cacheCreationCostPerMillion: "0.4",
  },
];

describe("PricingConfigPanel", () => {
  beforeEach(() => {
    useModelPricing.mockReturnValue({
      data: pricing,
      isLoading: false,
      error: null,
    });
    useDeleteModelPricing.mockReturnValue({
      mutate: vi.fn(),
      isPending: false,
    });
    getDefaultCostMultiplier.mockResolvedValue("1");
    getPricingModelSource.mockResolvedValue("response");
  });

  it("filters pricing rows by model ID or display name", async () => {
    render(<PricingConfigPanel />);

    const searchInput = await screen.findByPlaceholderText(
      "usage.pricingSearchPlaceholder",
    );
    await waitFor(() =>
      expect(screen.getByText("vendor/model-alpha")).toBeInTheDocument(),
    );
    expect(screen.getByText("vendor/model-alpha")).toBeInTheDocument();
    expect(screen.getByText("gpt-5.4")).toBeInTheDocument();

    fireEvent.change(searchInput, { target: { value: "openai" } });
    expect(screen.queryByText("vendor/model-alpha")).not.toBeInTheDocument();
    expect(screen.getByText("gpt-5.4")).toBeInTheDocument();

    fireEvent.change(searchInput, { target: { value: "MODEL-ALPHA" } });
    expect(screen.getByText("vendor/model-alpha")).toBeInTheDocument();
    expect(screen.queryByText("gpt-5.4")).not.toBeInTheDocument();
  });

  it("shows a no-results message when the keyword matches no model", async () => {
    render(<PricingConfigPanel />);

    const searchInput = await screen.findByPlaceholderText(
      "usage.pricingSearchPlaceholder",
    );
    fireEvent.change(searchInput, { target: { value: "does-not-exist" } });

    expect(
      screen.getByText("usage.noPricingSearchResults"),
    ).toBeInTheDocument();
    expect(screen.queryByText("vendor/model-alpha")).not.toBeInTheDocument();
  });
});
