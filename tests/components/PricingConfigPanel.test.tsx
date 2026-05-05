import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { PricingConfigPanel } from "@/components/usage/PricingConfigPanel";

const mocks = vi.hoisted(() => ({
  deleteMutate: vi.fn(),
  backfillMutateAsync: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
  t: (key: string, options?: Record<string, unknown>) =>
    options && "count" in options ? `${key}:${options.count}` : key,
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: mocks.t,
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: mocks.toastSuccess,
    error: mocks.toastError,
  },
}));

vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getDefaultCostMultiplier: vi.fn().mockResolvedValue("1"),
    getPricingModelSource: vi.fn().mockResolvedValue("response"),
    setDefaultCostMultiplier: vi.fn().mockResolvedValue(undefined),
    setPricingModelSource: vi.fn().mockResolvedValue(undefined),
  },
}));

vi.mock("@/lib/query/usage", () => ({
  useModelPricing: () => ({
    data: [
      {
        modelId: "priced-model-a",
        displayName: "Priced Model A",
        inputCostPerMillion: "5",
        outputCostPerMillion: "30",
        cacheReadCostPerMillion: "0.5",
        cacheCreationCostPerMillion: "5",
      },
    ],
    isLoading: false,
    error: null,
  }),
  useDeleteModelPricing: () => ({
    mutate: mocks.deleteMutate,
    isPending: false,
  }),
  useBackfillMissingUsageCosts: () => ({
    mutateAsync: mocks.backfillMutateAsync,
    isPending: false,
  }),
  useUpdateModelPricing: () => ({
    mutateAsync: vi.fn(),
    isPending: false,
  }),
}));

describe("PricingConfigPanel", () => {
  beforeEach(() => {
    mocks.deleteMutate.mockReset();
    mocks.backfillMutateAsync.mockReset();
    mocks.toastSuccess.mockReset();
    mocks.toastError.mockReset();
  });

  it("backfills historical zero-cost usage when the user clicks the pricing backfill button", async () => {
    mocks.backfillMutateAsync.mockResolvedValue({ backfilledCostRows: 2 });

    render(<PricingConfigPanel />);

    fireEvent.click(
      screen.getByRole("button", { name: "usage.backfillMissingCosts" }),
    );

    await waitFor(() =>
      expect(mocks.backfillMutateAsync).toHaveBeenCalledTimes(1),
    );
    expect(mocks.toastSuccess).toHaveBeenCalledWith(
      "usage.backfillMissingCostsDone:2",
      { closeButton: true },
    );
  });
});
