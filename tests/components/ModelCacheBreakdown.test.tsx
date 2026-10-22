import { screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import i18n from "i18next";
import { render } from "@testing-library/react";
import zh from "@/i18n/locales/zh.json";
import { ModelCacheBreakdown } from "@/components/usage/ModelCacheBreakdown";
import type { ModelStats } from "@/types/usage";

function model(
  name: string,
  fresh: number,
  read: number,
  write = 0,
): ModelStats {
  return {
    model: name,
    requestCount: 10,
    totalTokens: 100_000,
    totalCost: "0",
    avgCostPerRequest: "0",
    successRate: 100,
    freshInputTokens: fresh,
    cacheCreationTokens: write,
    cacheReadTokens: read,
  };
}

describe("ModelCacheBreakdown", () => {
  beforeEach(() => {
    i18n.addResourceBundle("zh", "translation", zh, true, true);
    i18n.changeLanguage("zh");
  });

  it("weights the summary by input tokens, excludes output and orders by input volume", () => {
    render(
      <ModelCacheBreakdown
        title="模型缓存"
        stats={[
          model("small-high-hit", 10, 90),
          model("large-no-hit", 800, 0, 100),
        ]}
      />,
    );
    expect(screen.getByText("9.0%")).toBeInTheDocument();
    const rows = within(screen.getByRole("list")).getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent("large-no-hit");
    expect(rows[0]).toHaveTextContent("0.0%");
    expect(rows[1]).toHaveTextContent("90.0%");
    // A 100-token cache write belongs in the denominator and has its own segment.
    expect(within(rows[0]).getByText("缓存写入 100")).toBeInTheDocument();
    expect(screen.queryByText("45.0%")).not.toBeInTheDocument();
  });

  it("keeps zero-input and missing data distinct and reports excluded models", () => {
    const missing = { ...model("missing", 0, 0), cacheReadTokens: undefined };
    render(
      <ModelCacheBreakdown
        title="模型缓存"
        stats={[model("zero-input", 0, 0), missing, model("known", 10, 90)]}
      />,
    );
    expect(screen.getByRole("note")).toHaveTextContent("1 个模型缺少缓存数据");
    const rows = within(screen.getByRole("list")).getAllByRole("listitem");
    expect(rows[1]).toHaveTextContent("zero-input");
    expect(rows[1]).toHaveTextContent("—");
    expect(rows[1]).toHaveTextContent("缓存命中 0");
    expect(rows[2]).toHaveTextContent("缓存数据缺失");
    expect(rows[2]).not.toHaveTextContent("缓存命中 0");
    expect(screen.getAllByText("90.0%")).toHaveLength(2);
  });

  it("never collapses long or unpriced model lists into an other bucket", () => {
    const stats = Array.from({ length: 8 }, (_, index) =>
      model(`model-${index}`, 10, 90),
    );
    render(<ModelCacheBreakdown title="模型缓存" stats={stats} />);
    const list = screen.getByRole("list");
    expect(within(list).getAllByRole("listitem")).toHaveLength(8);
    expect(list).toHaveAttribute("tabindex", "0");
    expect(within(list).getAllByRole("listitem")[0]).toHaveTextContent(
      "model-0",
    );
  });

  it("shows unavailable totals instead of fabricated zero tokens when all cache data is missing", () => {
    render(
      <ModelCacheBreakdown
        title="模型缓存"
        stats={[{ ...model("missing", 0, 0), freshInputTokens: undefined }]}
      />,
    );
    expect(screen.getAllByText("—")).toHaveLength(2);
    expect(screen.queryByText(/命中 0 \/ 输入 0/)).not.toBeInTheDocument();
    expect(screen.getAllByText("缓存数据缺失")).toHaveLength(2);
  });

  it("shows an empty state for a scope without model records", () => {
    render(<ModelCacheBreakdown title="模型缓存" stats={[]} />);
    expect(screen.getByText("暂无数据")).toBeInTheDocument();
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
  });
});
