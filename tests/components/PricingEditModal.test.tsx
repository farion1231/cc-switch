import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PricingEditModal } from "@/components/usage/PricingEditModal";
import { fetchModelsDevPricing } from "@/lib/modelsDevPricing";
import type { ModelPricing } from "@/types/usage";

const { updatePricingMutate } = vi.hoisted(() => ({
  updatePricingMutate: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: string | { defaultValue?: string }) =>
      typeof options === "string" ? options : (options?.defaultValue ?? key),
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock("@/lib/query/usage", () => ({
  useUpdateModelPricing: () => ({
    mutateAsync: updatePricingMutate,
    isPending: false,
  }),
}));

vi.mock("@/lib/modelsDevPricing", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/lib/modelsDevPricing")>();
  return {
    ...actual,
    fetchModelsDevPricing: vi.fn(),
  };
});

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

const model: ModelPricing = {
  modelId: "deepseek-v4",
  displayName: "DeepSeek V4",
  inputCostPerMillion: "1",
  outputCostPerMillion: "3",
  cacheReadCostPerMillion: "0.0028",
  cacheCreationCostPerMillion: "0",
};

const PRICE_FIELDS = [
  { id: "inputCost", label: "输入成本" },
  { id: "outputCost", label: "输出成本" },
  { id: "cacheReadCost", label: "缓存读取成本" },
  { id: "cacheCreationCost", label: "缓存写入成本" },
] as const;

function renderModal(onClose: () => void, isNew = false) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <PricingEditModal open model={model} isNew={isNew} onClose={onClose} />
    </QueryClientProvider>,
  );
}

describe("PricingEditModal", () => {
  it("all price inputs have step=0.0001", () => {
    renderModal(() => {});

    for (const { id } of PRICE_FIELDS) {
      const input = screen.getByLabelText(
        /每百万 tokens/ as unknown as string,
        {
          selector: `#${id}`,
        },
      ) as HTMLInputElement;
      expect(input).toHaveAttribute("step", "0.0001");
    }
  });

  it("accepts precise cache read cost like 0.0028", () => {
    renderModal(() => {});

    const cacheReadInput = document.getElementById(
      "cacheReadCost",
    ) as HTMLInputElement;
    expect(cacheReadInput.value).toBe("0.0028");
    expect(cacheReadInput.checkValidity()).toBe(true);
  });

  it("allows user to input sub-cent prices via change event", () => {
    renderModal(() => {}, true);

    const cacheReadInput = document.getElementById(
      "cacheReadCost",
    ) as HTMLInputElement;

    fireEvent.change(cacheReadInput, { target: { value: "0.0015" } });
    expect(cacheReadInput.value).toBe("0.0015");
  });

  it("fills the form from a models.dev selection without saving or closing the panel", async () => {
    const onClose = vi.fn();
    vi.mocked(fetchModelsDevPricing).mockResolvedValue({
      acme: {
        id: "acme",
        name: "Acme AI",
        models: {
          "acme-chat": {
            id: "acme-chat",
            name: "Acme Chat",
            release_date: "2025-01-01",
            cost: {
              input: 1.5,
              output: 3,
              cache_read: 0.3,
              cache_write: 0.6,
            },
          },
        },
      },
    });

    renderModal(onClose, true);

    fireEvent.click(
      screen.getByRole("button", { name: "从 models.dev 引用数据" }),
    );

    const row = await screen.findByText("Acme Chat");
    fireEvent.click(row);
    fireEvent.click(screen.getByRole("button", { name: "填入表单" }));

    expect((document.getElementById("modelId") as HTMLInputElement).value).toBe(
      "acme-chat",
    );
    expect(
      (document.getElementById("displayName") as HTMLInputElement).value,
    ).toBe("Acme Chat");
    expect(
      (document.getElementById("inputCost") as HTMLInputElement).value,
    ).toBe("1.5");
    expect(
      (document.getElementById("outputCost") as HTMLInputElement).value,
    ).toBe("3");
    expect(
      (document.getElementById("cacheReadCost") as HTMLInputElement).value,
    ).toBe("0.3");
    expect(
      (document.getElementById("cacheCreationCost") as HTMLInputElement).value,
    ).toBe("0.6");

    // 选择器已关闭（不再渲染），但"新增定价"面板保留
    expect(screen.queryByText("Acme Chat")).not.toBeInTheDocument();
    expect(document.getElementById("displayName")).not.toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    expect(updatePricingMutate).not.toHaveBeenCalled();
  });
});
