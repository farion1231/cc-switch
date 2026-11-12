import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import {
  PricingConfigPanel,
  filterPricingByPrefix,
} from "@/components/usage/PricingConfigPanel";
import type { ModelPricing } from "@/types/usage";

// t 必须是稳定引用：useBillingSources 的 effect 依赖 [t]，每次渲染换新函数会无限重跑。
const i18nMock = vi.hoisted(() => ({
  t: (key: string, options?: unknown) => {
    if (options && typeof options === "object") {
      const values = Object.values(options as Record<string, unknown>).map(
        String,
      );
      return values.length ? `${key}:${values.join(",")}` : key;
    }
    return key;
  },
  i18n: { resolvedLanguage: "en", language: "en" },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => i18nMock,
}));

const pricing = (modelId: string, displayName = ""): ModelPricing => ({
  modelId,
  displayName,
  inputCostPerMillion: "1",
  outputCostPerMillion: "2",
  cacheReadCostPerMillion: "0.1",
  cacheCreationCostPerMillion: "1.25",
});

const ROWS: ModelPricing[] = [
  pricing("claude-3-5-haiku-20241022", "Claude 3.5 Haiku"),
  pricing("claude-3-5-sonnet-20241022", "Claude 3.5 Sonnet"),
  pricing("gpt-5-mini", "GPT-5 mini"),
  pricing("gpt-5", "GPT-5"),
];

vi.mock("@/lib/query/usage", () => ({
  useModelPricing: () => ({ data: ROWS, isLoading: false, error: null }),
  useDeleteModelPricing: () => ({ mutate: vi.fn(), isPending: false }),
}));

vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getPricingModelSource: vi.fn().mockResolvedValue("response"),
    setPricingModelSource: vi.fn(),
  },
}));

vi.mock("@/components/usage/ModelsDevAutoSyncPanel", () => ({
  ModelsDevAutoSyncPanel: () => null,
}));

vi.mock("@/components/usage/ModelsDevPickerDialog", () => ({
  ModelsDevPickerDialog: () => null,
}));

vi.mock("@/components/usage/PricingEditModal", () => ({
  PricingEditModal: ({
    model,
    isNew,
  }: {
    model: ModelPricing;
    isNew?: boolean;
  }) => (
    <div data-testid="pricing-edit-modal">
      {isNew ? "new" : "edit"}:{model.modelId}
    </div>
  ),
}));

const modelIdsInTable = () =>
  within(screen.getByRole("table"))
    .getAllByRole("row")
    .slice(1)
    .map((row) => row.querySelector(".font-mono")?.textContent);

describe("filterPricingByPrefix", () => {
  it("returns every row for a blank query", () => {
    expect(filterPricingByPrefix(ROWS, "  ")).toHaveLength(ROWS.length);
  });

  it("matches the model ID or display name by prefix, ignoring case", () => {
    expect(
      filterPricingByPrefix(ROWS, "CLAUDE 3.5 s").map((r) => r.modelId),
    ).toEqual(["claude-3-5-sonnet-20241022"]);
    expect(
      filterPricingByPrefix(ROWS, "claude-3-5").map((r) => r.modelId),
    ).toEqual(["claude-3-5-haiku-20241022", "claude-3-5-sonnet-20241022"]);
  });

  it("matches at the start of any word in the ID or display name", () => {
    expect(filterPricingByPrefix(ROWS, "sonnet").map((r) => r.modelId)).toEqual(
      ["claude-3-5-sonnet-20241022"],
    );
    expect(filterPricingByPrefix(ROWS, "mini").map((r) => r.modelId)).toEqual([
      "gpt-5-mini",
    ]);
    expect(
      filterPricingByPrefix(ROWS, "3.5 haiku").map((r) => r.modelId),
    ).toEqual(["claude-3-5-haiku-20241022"]);
  });

  it("does not match in the middle of a word", () => {
    expect(filterPricingByPrefix(ROWS, "net")).toEqual([]);
    expect(filterPricingByPrefix(ROWS, "laude")).toEqual([]);
    expect(filterPricingByPrefix(ROWS, "ini")).toEqual([]);
  });

  it("moves an exact model ID match to the front", () => {
    expect(filterPricingByPrefix(ROWS, "gpt-5").map((r) => r.modelId)).toEqual([
      "gpt-5",
      "gpt-5-mini",
    ]);
  });
});

describe("PricingConfigPanel search", () => {
  it("filters the table as the user types", async () => {
    const user = userEvent.setup();
    render(<PricingConfigPanel />);

    await user.type(
      screen.getByRole("textbox", { name: "usage.pricing.searchPlaceholder" }),
      "claude",
    );

    expect(modelIdsInTable()).toEqual([
      "claude-3-5-haiku-20241022",
      "claude-3-5-sonnet-20241022",
    ]);
  });

  it("opens the first match for editing on Enter", async () => {
    const user = userEvent.setup();
    render(<PricingConfigPanel />);

    await user.type(
      screen.getByRole("textbox", { name: "usage.pricing.searchPlaceholder" }),
      "gpt-5{Enter}",
    );

    expect(screen.getByTestId("pricing-edit-modal")).toHaveTextContent(
      "edit:gpt-5",
    );
  });

  it("moves focus off the search box once Enter opens the editor", async () => {
    const user = userEvent.setup();
    render(<PricingConfigPanel />);

    const search = screen.getByRole("textbox", {
      name: "usage.pricing.searchPlaceholder",
    });
    await user.type(search, "sonnet{Enter}");

    expect(screen.getByTestId("pricing-edit-modal")).toHaveTextContent(
      "edit:claude-3-5-sonnet-20241022",
    );
    expect(search).not.toHaveFocus();
  });

  it("offers to add pricing for the query when nothing matches", async () => {
    const user = userEvent.setup();
    render(<PricingConfigPanel />);

    await user.type(
      screen.getByRole("textbox", { name: "usage.pricing.searchPlaceholder" }),
      "deepseek-v4",
    );
    expect(
      screen.getByText("usage.pricing.noSearchMatch:deepseek-v4"),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", {
        name: "usage.pricing.addForQuery:deepseek-v4",
      }),
    );
    expect(screen.getByTestId("pricing-edit-modal")).toHaveTextContent(
      "new:deepseek-v4",
    );
  });
});
