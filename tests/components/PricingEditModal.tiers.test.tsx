import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { formatTieredUnitPrice } from "@/components/usage/PricingConfigPanel";
import { PricingEditModal } from "@/components/usage/PricingEditModal";
import type { ModelPricing } from "@/types/usage";

const mocks = vi.hoisted(() => ({
  mutateAsync: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: string | { defaultValue?: string }) =>
      typeof options === "string" ? options : (options?.defaultValue ?? key),
  }),
}));

vi.mock("@/lib/toast", () => ({
  toast: { success: vi.fn(), error: mocks.toastError },
}));

vi.mock("@/lib/query/usage", () => ({
  useUpdateModelPricing: () => ({
    mutateAsync: mocks.mutateAsync,
    isPending: false,
  }),
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    children,
    footer,
  }: {
    children: React.ReactNode;
    footer?: React.ReactNode;
  }) => (
    <div>
      {children}
      {footer}
    </div>
  ),
}));

const flat: ModelPricing = {
  modelId: "claude-haiku-5-5",
  displayName: "Claude Haiku 5.5",
  inputCostPerMillion: "0.10",
  outputCostPerMillion: "0.50",
  cacheReadCostPerMillion: "0.01",
  cacheCreationCostPerMillion: "0.125",
  above100k: null,
};

const upperTier = {
  inputCostPerMillion: "0.50",
  outputCostPerMillion: "2.50",
  cacheReadCostPerMillion: "0.05",
  cacheCreationCostPerMillion: "0.625",
};

const TIER_INPUT_IDS = [
  "inputCostAbove100k",
  "outputCostAbove100k",
  "cacheReadCostAbove100k",
  "cacheCreationCostAbove100k",
];

const setTierInputs = (values: string[]) =>
  TIER_INPUT_IDS.forEach((id, index) =>
    fireEvent.change(document.getElementById(id)!, {
      target: { value: values[index] },
    }),
  );

const submit = () => fireEvent.submit(document.getElementById("pricing-form")!);

const savedEntry = async () => {
  await waitFor(() => expect(mocks.mutateAsync).toHaveBeenCalledOnce());
  return mocks.mutateAsync.mock.calls[0][0] as ModelPricing;
};

beforeEach(() => {
  vi.clearAllMocks();
  mocks.mutateAsync.mockResolvedValue(undefined);
});
afterEach(cleanup);

describe("PricingEditModal >100K prices", () => {
  it("leaves the field out when the model had no upper tier and the inputs stay empty", async () => {
    render(<PricingEditModal open model={flat} onClose={() => {}} />);
    submit();
    expect(await savedEntry()).not.toHaveProperty("above100k");
  });

  it("sends the four prices when all are filled", async () => {
    render(<PricingEditModal open model={flat} onClose={() => {}} />);
    setTierInputs(["0.50", "2.50", "0.05", "0.625"]);
    submit();
    expect((await savedEntry()).above100k).toEqual(upperTier);
  });

  it("sends null when all four existing prices are cleared", async () => {
    render(
      <PricingEditModal
        open
        model={{ ...flat, above100k: upperTier }}
        onClose={() => {}}
      />,
    );
    setTierInputs(["", "", "", ""]);
    submit();
    expect((await savedEntry()).above100k).toBeNull();
  });

  it("rejects a partly filled upper tier", () => {
    render(<PricingEditModal open model={flat} onClose={() => {}} />);
    setTierInputs(["0.50", "", "", ""]);
    submit();
    expect(mocks.toastError).toHaveBeenCalledWith(
      "超过 100K 的四项单价要么都填，要么都留空",
    );
    expect(mocks.mutateAsync).not.toHaveBeenCalled();
  });
});

describe("formatTieredUnitPrice", () => {
  it("shows the upper price only when the model has an upper tier", () => {
    expect(
      formatTieredUnitPrice(
        { ...flat, above100k: upperTier },
        "inputCostPerMillion",
      ),
    ).toBe("$0.10 / $0.50");
    expect(formatTieredUnitPrice(flat, "cacheCreationCostPerMillion")).toBe(
      "$0.125",
    );
  });
});
