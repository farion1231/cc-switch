import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ModelStatsTable } from "@/components/usage/ModelStatsTable";
import type { ModelStats } from "@/types/usage";

const useModelStatsMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useModelStats: (...args: unknown[]) => useModelStatsMock(...args),
}));

const stat = (overrides: Partial<ModelStats> = {}): ModelStats => ({
  model: "example-model",
  requestCount: 1,
  totalTokens: 1_000,
  totalCost: "1",
  avgCostPerRequest: "1",
  successRate: 100,
  ...overrides,
});

describe("ModelStatsTable", () => {
  it("shows six default columns and preserves request ordering, cost detail and estimated speed", () => {
    useModelStatsMock.mockReturnValue({
      isLoading: false,
      data: [
        stat({ model: "low-volume" }),
        stat({
          model: "high-volume",
          requestCount: 5,
          totalCost: "2.5",
          avgCostPerRequest: "0.5",
          estSpeedOutputTokens: 2_000,
          estSpeedDurationMs: 20_000,
        }),
      ],
    });
    render(<ModelStatsTable range={{ preset: "7d" }} refreshIntervalMs={0} />);

    expect(screen.getAllByRole("columnheader")).toHaveLength(6);
    const rows = screen.getAllByRole("row").slice(1);
    expect(rows[0].children).toHaveLength(6);
    expect(rows[0]).toHaveTextContent("high-volume");
    expect(rows[0].lastElementChild).toHaveTextContent("≈100tok/s");
    expect(rows[1]).toHaveTextContent("low-volume");
    expect(rows[1].lastElementChild).toHaveTextContent("—");
    expect(screen.getByText("$2.50")).toHaveAttribute(
      "title",
      "$2.500000 · usage.avgCost $0.5000",
    );
  });

  it("hides selected metrics in headers and rows while reducing the minimum width", () => {
    useModelStatsMock.mockReturnValue({ isLoading: false, data: [stat()] });
    const { rerender } = render(
      <ModelStatsTable range={{ preset: "7d" }} refreshIntervalMs={0} />,
    );
    const initialWidth = parseFloat(screen.getByRole("table").style.minWidth);
    rerender(
      <ModelStatsTable
        range={{ preset: "7d" }}
        refreshIntervalMs={0}
        columnVisibility={{
          requests: false,
          tokens: false,
          successRate: false,
        }}
      />,
    );

    expect(screen.getAllByRole("columnheader")).toHaveLength(3);
    expect(
      screen.queryByRole("columnheader", { name: "usage.requests" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("columnheader", { name: "usage.tokens" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("columnheader", { name: "usage.successRate" }),
    ).not.toBeInTheDocument();
    expect(screen.getAllByRole("cell")).toHaveLength(3);
    expect(screen.getByText("$1.00")).toBeInTheDocument();
    expect(parseFloat(screen.getByRole("table").style.minWidth)).toBeLessThan(
      initialWidth,
    );
  });

  it.each([false, true])(
    "always keeps the model identifier (empty: %s)",
    (empty) => {
      useModelStatsMock.mockReturnValue({
        isLoading: false,
        data: empty ? [] : [stat()],
      });
      render(
        <ModelStatsTable
          range={{ preset: "7d" }}
          refreshIntervalMs={0}
          columnVisibility={{
            model: false,
            requests: false,
            tokens: false,
            cost: false,
            successRate: false,
            speed: false,
          }}
        />,
      );
      expect(screen.getAllByRole("columnheader")).toHaveLength(1);
      expect(screen.getByRole("columnheader")).toHaveTextContent("usage.model");
      expect(screen.getAllByRole("cell")).toHaveLength(1);
      if (empty)
        expect(screen.getByRole("cell")).toHaveAttribute("colspan", "1");
      else expect(screen.getByRole("cell")).toHaveTextContent("example-model");
    },
  );

  it("updates the empty-state span when column visibility changes", () => {
    useModelStatsMock.mockReturnValue({ isLoading: false, data: [] });
    const { rerender } = render(
      <ModelStatsTable range={{ preset: "7d" }} refreshIntervalMs={0} />,
    );
    expect(screen.getByRole("cell")).toHaveAttribute("colspan", "6");
    rerender(
      <ModelStatsTable
        range={{ preset: "7d" }}
        refreshIntervalMs={0}
        columnVisibility={{ cost: false, speed: false }}
      />,
    );
    expect(screen.getAllByRole("columnheader")).toHaveLength(4);
    expect(screen.getByRole("cell")).toHaveAttribute("colspan", "4");
  });

  it("keeps the selected page and query filters when a column is hidden", () => {
    useModelStatsMock.mockReturnValue({
      isLoading: false,
      data: Array.from({ length: 21 }, (_, index) =>
        stat({ model: `model-${index}`, requestCount: 21 - index }),
      ),
    });
    const props = {
      range: { preset: "7d" as const },
      appType: "codex",
      providerName: "Example",
      model: "model",
      refreshIntervalMs: 15_000,
    };
    const { rerender } = render(<ModelStatsTable {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "usage.nextPage" }));
    expect(screen.getByText("model-20")).toBeInTheDocument();
    rerender(
      <ModelStatsTable {...props} columnVisibility={{ requests: false }} />,
    );
    expect(screen.getByText("model-20")).toBeInTheDocument();
    expect(screen.queryByText("model-0")).not.toBeInTheDocument();
    expect(useModelStatsMock).toHaveBeenLastCalledWith(
      { preset: "7d" },
      { appType: "codex", providerName: "Example", model: "model" },
      { refetchInterval: 15_000 },
    );
  });
});
