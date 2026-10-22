import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ModelStatsTable } from "@/components/usage/ModelStatsTable";
import type { ModelStats } from "@/types/usage";

const { useModelStatsMock, useSettingsQueryMock, refetchMock } = vi.hoisted(
  () => ({
    useModelStatsMock: vi.fn(),
    useSettingsQueryMock: vi.fn(),
    refetchMock: vi.fn(),
  }),
);

vi.mock("@/lib/query/usage", () => ({
  useModelStats: (...args: unknown[]) => useModelStatsMock(...args),
}));

vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => useSettingsQueryMock(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: unknown) =>
      options && typeof options === "object" && "total" in options
        ? `${key}:${options.total}`
        : key,
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));

const alpha: ModelStats = {
  model: "alpha",
  requestCount: 2,
  totalTokens: 75,
  totalCost: "1.000000",
  avgCostPerRequest: "0.500000",
  successRate: 98.4,
  speedOutputTokens: 500,
  speedGenerationMs: 10000,
  firstTokenMedianMs: 400,
};
const beta: ModelStats = {
  model: "beta",
  requestCount: 1,
  totalTokens: 25,
  totalCost: "3.000000",
  avgCostPerRequest: "3.000000",
  successRate: 100,
};
// Deliberately not sorted by requests: the table preserves main's ordering.
const stats = [beta, alpha];
const defaultProps: ComponentProps<typeof ModelStatsTable> = {
  range: { preset: "7d" },
  refreshIntervalMs: 0,
};
const chartRect = {
  x: 0,
  y: 0,
  top: 0,
  left: 0,
  right: 760,
  bottom: 280,
  width: 760,
  height: 280,
  toJSON: () => ({}),
};

function setQuery(overrides: Record<string, unknown> = {}) {
  useModelStatsMock.mockReturnValue({
    data: stats,
    isLoading: false,
    isPlaceholderData: false,
    isFetching: false,
    isError: false,
    refetch: refetchMock,
    ...overrides,
  });
}

function disclosure() {
  return screen.getByRole("button", {
    name: "usage.modelDistribution.tokenShare / usage.modelDistribution.costShare",
  });
}

function legend(model: string) {
  return screen.getByRole("button", {
    name: new RegExp(`^${model},`),
  });
}

function tableRow(model: string) {
  return within(screen.getByRole("table")).getByText(model).closest("tr")!;
}

function sectors(container: HTMLElement) {
  return Array.from(
    container.querySelectorAll<SVGPathElement>("path.recharts-sector"),
  );
}

describe("ModelStatsTable distribution", () => {
  beforeEach(() => {
    useModelStatsMock.mockReset();
    useSettingsQueryMock.mockReset();
    refetchMock.mockReset().mockResolvedValue(undefined);
    setQuery();
    useSettingsQueryMock.mockReturnValue({
      data: { usageDisplay: { currency: "USD" } },
    });

    // Give the real ResponsiveContainer a measured layout in jsdom.
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
      chartRect,
    );
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(private readonly callback: ResizeObserverCallback) {}
        observe(target: Element) {
          this.callback(
            [{ target, contentRect: chartRect } as ResizeObserverEntry],
            this as unknown as ResizeObserver,
          );
        }
        unobserve() {}
        disconnect() {}
      },
    );
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("passes all filters to the shared query and keeps the sorted performance table visible while charts are collapsed", () => {
    render(
      <ModelStatsTable
        {...defaultProps}
        appType="codex"
        providerName="source"
        model="gpt"
      />,
    );

    expect(useModelStatsMock).toHaveBeenCalledWith(
      { preset: "7d" },
      { appType: "codex", providerName: "source", model: "gpt" },
      { refetchInterval: false },
    );
    expect(disclosure()).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("button", { name: /^alpha,/ })).toBeNull();
    const rows = within(screen.getByRole("table")).getAllByRole("row");
    expect(within(rows[1]).getByText("alpha")).toBeInTheDocument();
    expect(within(rows[2]).getByText("beta")).toBeInTheDocument();
    const cells = within(rows[1]).getAllByRole("cell");
    expect(cells).toHaveLength(9);
    expect(cells[3]).toHaveTextContent(/^75.0%$/);
    expect(cells[5]).toHaveTextContent(/^25.0%$/);
    expect(cells[6]).toHaveTextContent("98.4%");
    expect(cells[7]).toHaveTextContent("50tok/s");
    expect(cells[8]).toHaveTextContent("0.4s");
    expect(cells[4]).toHaveAttribute(
      "title",
      "$1.000000 · usage.avgCost $0.5000",
    );
    expect(
      screen.getByRole("columnheader", { name: "usage.successRate" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: /^usage.speed/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: /^usage.firstToken/ }),
    ).toBeInTheDocument();
  });

  it("uses all 21 models as chart and share denominators across the 20-row pages", () => {
    const allStats = Array.from({ length: 21 }, (_, index) => ({
      ...alpha,
      model: `model-${index + 1}`,
      requestCount: index + 1,
      totalTokens: 10,
      totalCost: "1",
    }));
    setQuery({ data: allStats });
    render(<ModelStatsTable {...defaultProps} />);

    expect(within(screen.getByRole("table")).getAllByRole("row")).toHaveLength(
      21,
    );
    expect(within(screen.getByRole("table")).queryByText("model-1")).toBeNull();
    const firstPageCells = within(tableRow("model-21")).getAllByRole("cell");
    expect(firstPageCells[3]).toHaveTextContent(/^4.8%$/);
    expect(firstPageCells[5]).toHaveTextContent(/^4.8%$/);
    fireEvent.click(disclosure());
    expect(
      within(screen.getByRole("group", { name: "usage.model" })).getAllByRole(
        "button",
      ),
    ).toHaveLength(21);
    expect(screen.getByTitle("210")).toHaveTextContent("210");
    expect(screen.getByTitle("$21.000000")).toHaveTextContent("$21.00");

    fireEvent.click(screen.getByRole("button", { name: "usage.nextPage" }));
    const lastPageCells = within(tableRow("model-1")).getAllByRole("cell");
    expect(lastPageCells[3]).toHaveTextContent(/^4.8%$/);
    expect(lastPageCells[5]).toHaveTextContent(/^4.8%$/);
    expect(within(screen.getByRole("table")).getAllByRole("row")).toHaveLength(
      2,
    );
    expect(screen.getByTitle("210")).toHaveTextContent("210");
    expect(screen.getByTitle("$21.000000")).toHaveTextContent("$21.00");
  });

  it("renders complete model IDs in the real SVG labels and shared legend", async () => {
    const model = "gpt-6-astra-202609";
    setQuery({ data: [{ ...alpha, model }] });
    const { container } = render(<ModelStatsTable {...defaultProps} />);
    fireEvent.click(disclosure());

    await waitFor(() => expect(sectors(container)).toHaveLength(2));
    const labels = Array.from(container.querySelectorAll("svg text"));
    expect(labels).toHaveLength(2);
    for (const label of labels) {
      expect(label.lastChild?.textContent).toBe(`${model} 100.0%`);
    }
    expect(within(legend(model)).getByText(model)).toBeInTheDocument();
  });

  it("omits a smaller outside label that collides after pie padding while keeping both models in the legend", async () => {
    const values = [134, 172, 16, 22, 99];
    setQuery({
      data: values.map((value, index) => ({
        ...alpha,
        model: `gpt-6-sol-${String(index).padStart(4, "0")}`,
        totalTokens: value,
        totalCost: String(value),
      })),
    });
    const { container } = render(<ModelStatsTable {...defaultProps} />);
    fireEvent.click(disclosure());
    await waitFor(() => expect(sectors(container)).toHaveLength(10));

    const labels = Array.from(
      container.querySelectorAll("svg text"),
      (label) => label.lastChild?.textContent,
    );
    expect(
      labels.filter((label) => label === "gpt-6-sol-0003 5.0%"),
    ).toHaveLength(2);
    expect(
      labels.filter((label) => label === "gpt-6-sol-0002 3.6%"),
    ).toHaveLength(0);
    expect(
      within(legend("gpt-6-sol-0002")).getByText("gpt-6-sol-0002"),
    ).toBeInTheDocument();
    expect(
      within(legend("gpt-6-sol-0003")).getByText("gpt-6-sol-0003"),
    ).toBeInTheDocument();
  });

  it("hides both SVG sectors with the keyboard without changing angles, totals or table shares and can restore all", async () => {
    const user = userEvent.setup();
    const { container } = render(<ModelStatsTable {...defaultProps} />);
    fireEvent.click(disclosure());
    await waitFor(() => expect(sectors(container)).toHaveLength(4));
    const paths = sectors(container).map((path) => path.getAttribute("d"));
    legend("alpha").focus();
    await user.keyboard("{Enter}");

    expect(legend("alpha")).toHaveAttribute("aria-pressed", "false");
    expect(sectors(container).map((path) => path.getAttribute("d"))).toEqual(
      paths,
    );
    const hidden = sectors(container).filter(
      (path) => path.getAttribute("fill") === "transparent",
    );
    expect(hidden).toHaveLength(2);
    for (const path of hidden)
      expect(path).toHaveAttribute("pointer-events", "none");
    expect(
      Array.from(container.querySelectorAll("svg text")).some((label) =>
        label.textContent?.includes("alpha"),
      ),
    ).toBe(false);
    expect(screen.getByTitle("100")).toHaveTextContent("100");
    expect(screen.getByTitle("$4.000000")).toHaveTextContent("$4.00");
    expect(within(tableRow("alpha")).getAllByRole("cell")[3]).toHaveTextContent(
      "75.0%",
    );

    fireEvent.click(disclosure());
    fireEvent.click(disclosure());
    expect(legend("alpha")).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(legend("beta"));
    expect(
      screen.getAllByText("usage.modelDistribution.allHidden"),
    ).toHaveLength(1);
    fireEvent.click(
      screen.getByRole("button", { name: "usage.modelDistribution.showAll" }),
    );
    expect(legend("alpha")).toHaveAttribute("aria-pressed", "true");
    expect(legend("beta")).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByText("usage.modelDistribution.allHidden")).toBeNull();
  });

  it("resets legend visibility for each time-range field and app, provider and model filter", () => {
    let props: ComponentProps<typeof ModelStatsTable> = {
      range: {
        preset: "custom",
        customStartDate: 100,
        customEndDate: 200,
        liveEndTime: false,
      },
      appType: "codex",
      providerName: "source",
      model: "gpt",
      refreshIntervalMs: 30000,
    };
    const { rerender } = render(<ModelStatsTable {...props} />);
    fireEvent.click(disclosure());
    const changes: Array<(current: typeof props) => typeof props> = [
      (current) => ({
        ...current,
        range: { ...current.range, customStartDate: 110 },
      }),
      (current) => ({
        ...current,
        range: { ...current.range, customEndDate: 220 },
      }),
      (current) => ({
        ...current,
        range: { ...current.range, liveEndTime: true },
      }),
      (current) => ({ ...current, range: { ...current.range, preset: "7d" } }),
      (current) => ({ ...current, appType: "claude" }),
      (current) => ({ ...current, providerName: "other" }),
      (current) => ({ ...current, model: "gpt-6" }),
    ];
    for (const change of changes) {
      fireEvent.click(legend("alpha"));
      expect(legend("alpha")).toHaveAttribute("aria-pressed", "false");
      props = change(props);
      rerender(<ModelStatsTable {...props} />);
      expect(legend("alpha")).toHaveAttribute("aria-pressed", "true");
      expect(useModelStatsMock).toHaveBeenLastCalledWith(
        props.range,
        {
          appType: props.appType,
          providerName: props.providerName,
          model: props.model,
        },
        { refetchInterval: 30000 },
      );
    }
  });

  it("preserves hidden models and existing content during same-scope refresh and reports a refresh failure", () => {
    const { rerender } = render(
      <ModelStatsTable {...defaultProps} refreshIntervalMs={30000} />,
    );
    fireEvent.click(disclosure());
    fireEvent.click(legend("alpha"));
    setQuery({ isFetching: true });
    rerender(
      <ModelStatsTable
        {...defaultProps}
        range={{ preset: "7d" }}
        refreshIntervalMs={30000}
      />,
    );
    expect(tableRow("alpha")).toBeInTheDocument();
    expect(legend("alpha")).toHaveAttribute("aria-pressed", "false");

    setQuery({
      data: stats.map((stat) => ({
        ...stat,
        totalTokens: stat.totalTokens * 2,
      })),
    });
    rerender(<ModelStatsTable {...defaultProps} refreshIntervalMs={30000} />);
    expect(screen.getByTitle("200")).toHaveTextContent("200");
    expect(legend("alpha")).toHaveAttribute("aria-pressed", "false");
    setQuery({ isError: true });
    rerender(<ModelStatsTable {...defaultProps} refreshIntervalMs={30000} />);
    expect(tableRow("alpha")).toBeInTheDocument();
    expect(legend("alpha")).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("status")).toHaveTextContent(
      "usage.modelDistribution.refreshError",
    );
  });

  it("shows loading placeholders and never presents previous-scope data as the new filter result", () => {
    setQuery({ data: undefined, isLoading: true });
    const { container, rerender } = render(
      <ModelStatsTable {...defaultProps} />,
    );
    expect(screen.queryByRole("table")).toBeNull();
    expect(container.querySelector(".animate-pulse")).toBeInTheDocument();
    setQuery();
    rerender(<ModelStatsTable {...defaultProps} />);
    fireEvent.click(disclosure());
    expect(legend("alpha")).toBeInTheDocument();

    setQuery({ isPlaceholderData: true, isFetching: true });
    rerender(<ModelStatsTable {...defaultProps} appType="claude" />);
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.queryByText("alpha")).toBeNull();
    expect(sectors(container)).toHaveLength(0);
    expect(container.querySelector(".animate-pulse")).toBeInTheDocument();
    setQuery({ data: [{ ...alpha, model: "new-scope" }] });
    rerender(<ModelStatsTable {...defaultProps} appType="claude" />);
    expect(tableRow("new-scope")).toBeInTheDocument();
    expect(within(screen.getByRole("table")).queryByText("alpha")).toBeNull();
  });

  it("offers retry after the first request fails", () => {
    setQuery({ data: undefined, isError: true });
    render(<ModelStatsTable {...defaultProps} />);
    expect(
      screen.getByText("usage.modelDistribution.loadError"),
    ).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "usage.modelDistribution.retry" }),
    );
    expect(refetchMock).toHaveBeenCalledTimes(1);
  });

  it("keeps zero-value models visible and handles each zero-total metric independently", () => {
    setQuery({ data: stats.map((stat) => ({ ...stat, totalCost: "0" })) });
    const { rerender } = render(<ModelStatsTable {...defaultProps} />);
    fireEvent.click(disclosure());
    expect(
      screen.getByText("usage.modelDistribution.zeroCost"),
    ).toBeInTheDocument();
    expect(screen.queryByText("usage.modelDistribution.zeroTokens")).toBeNull();
    expect(legend("alpha")).toHaveAccessibleName(
      "alpha, usage.modelDistribution.tokenShare: 75.0%, usage.modelDistribution.costShare: —",
    );
    expect(within(tableRow("alpha")).getAllByRole("cell")[5]).toHaveTextContent(
      "—",
    );

    setQuery({ data: stats.map((stat) => ({ ...stat, totalTokens: 0 })) });
    rerender(<ModelStatsTable {...defaultProps} />);
    expect(
      screen.getByText("usage.modelDistribution.zeroTokens"),
    ).toBeInTheDocument();
    expect(screen.queryByText("usage.modelDistribution.zeroCost")).toBeNull();
    expect(legend("alpha")).toHaveAccessibleName(
      "alpha, usage.modelDistribution.tokenShare: —, usage.modelDistribution.costShare: 25.0%",
    );
    expect(within(tableRow("alpha")).getAllByRole("cell")[3]).toHaveTextContent(
      "—",
    );
  });

  it("converts chart, table and tooltip amounts when currency changes without changing shares or angles", async () => {
    const { container, rerender } = render(
      <ModelStatsTable {...defaultProps} />,
    );
    fireEvent.click(disclosure());
    await waitFor(() => expect(sectors(container)).toHaveLength(4));
    const paths = sectors(container).map((path) => path.getAttribute("d"));
    expect(screen.getByTitle("$4.000000")).toHaveTextContent("$4.00");
    expect(within(tableRow("alpha")).getAllByRole("cell")[4]).toHaveTextContent(
      "$1.00",
    );
    fireEvent.mouseEnter(container.querySelectorAll(".recharts-pie-sector")[3]);
    expect(await screen.findByText("$1.0000")).toHaveAttribute(
      "title",
      "$1.000000",
    );
    const shares = legend("alpha").getAttribute("aria-label");

    useSettingsQueryMock.mockReturnValue({
      data: { usageDisplay: { currency: "CNY", exchangeRates: { CNY: 2 } } },
    });
    rerender(<ModelStatsTable {...defaultProps} />);
    expect(screen.getByTitle("¥8.00 ($4.000000)")).toHaveTextContent("¥8.00");
    expect(within(tableRow("alpha")).getAllByRole("cell")[4]).toHaveTextContent(
      "¥2.00",
    );
    expect(within(tableRow("alpha")).getAllByRole("cell")[4]).toHaveAttribute(
      "title",
      "¥2.00 ($1.000000) · usage.avgCost ¥1.00",
    );
    expect(screen.getByTitle("¥2.00 ($1.000000)")).toHaveTextContent("¥2.00");
    expect(legend("alpha")).toHaveAttribute("aria-label", shares);
    expect(sectors(container).map((path) => path.getAttribute("d"))).toEqual(
      paths,
    );
    expect(within(tableRow("alpha")).getAllByRole("cell")[3]).toHaveTextContent(
      "75.0%",
    );
    expect(within(tableRow("alpha")).getAllByRole("cell")[5]).toHaveTextContent(
      "25.0%",
    );
  });

  it("shows one empty state when no models match", () => {
    setQuery({ data: [] });
    render(<ModelStatsTable {...defaultProps} />);
    expect(screen.getAllByText("usage.noData")).toHaveLength(1);
    expect(
      screen.queryByRole("button", {
        name: /^usage.modelDistribution.tokenShare/,
      }),
    ).toBeNull();
  });
});
