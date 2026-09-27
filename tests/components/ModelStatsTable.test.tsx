import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ModelStatsTable } from "@/components/usage/ModelStatsTable";
import type { ModelStats } from "@/types/usage";

const useModelStatsMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/query/usage", () => ({
  useModelStats: (...args: unknown[]) => useModelStatsMock(...args),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));

// Keep this test focused on our query, table and legend behavior.
vi.mock("recharts", () => {
  const Frame = ({ children }: { children?: ReactNode }) => (
    <div>{children}</div>
  );
  return {
    ResponsiveContainer: Frame,
    PieChart: Frame,
    Pie: ({
      children,
      data,
      label,
    }: {
      children?: ReactNode;
      data?: Array<{ model: string }>;
      label?: (props: {
        payload: { model: string };
        x: number;
        y: number;
        cx: number;
        cy: number;
        outerRadius: number;
        textAnchor: "start";
      }) => ReactNode;
    }) => (
      <div>
        <svg>
          {data?.[0] &&
            label?.({
              payload: data[0],
              x: 220,
              y: 80,
              cx: 300,
              cy: 140,
              outerRadius: 90,
              textAnchor: "start",
            })}
        </svg>
        {children}
      </div>
    ),
    Cell: () => null,
    Tooltip: () => null,
  };
});

const stats: ModelStats[] = [
  {
    model: "alpha",
    requestCount: 2,
    totalTokens: 75,
    totalCost: "1.000000",
    avgCostPerRequest: "0.500000",
  },
  {
    model: "beta",
    requestCount: 1,
    totalTokens: 25,
    totalCost: "3.000000",
    avgCostPerRequest: "3.000000",
  },
];

describe("ModelStatsTable distribution", () => {
  beforeEach(() => {
    useModelStatsMock.mockReset();
    useModelStatsMock.mockReturnValue({
      data: stats,
      isLoading: false,
      isError: false,
      refetch: vi.fn(),
    });
  });

  it("uses one filtered model query for chart totals and table shares", () => {
    render(
      <ModelStatsTable
        range={{ preset: "7d" }}
        appType="codex"
        providerName="source"
        model="gpt"
        refreshIntervalMs={0}
      />,
    );

    expect(useModelStatsMock).toHaveBeenCalledWith(
      { preset: "7d" },
      { appType: "codex", providerName: "source", model: "gpt" },
      { refetchInterval: false },
    );
    const expand = screen.getByRole("button", {
      name: "usage.modelDistribution.tokenShare / usage.modelDistribution.costShare",
    });
    expect(expand).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("button", { name: /^alpha,/ })).toBeNull();

    const rows = within(screen.getByRole("table")).getAllByRole("row");
    expect(within(rows[1]).getByText("alpha")).toBeInTheDocument();
    expect(within(rows[1]).getByText("75.0%")).toBeInTheDocument();
    expect(within(rows[1]).getByText("25.0%")).toBeInTheDocument();
    expect(within(rows[2]).getByText("25.0%")).toBeInTheDocument();
    expect(within(rows[2]).getByText("75.0%")).toBeInTheDocument();
  });

  it("renders full model IDs outside both charts and in the legend", () => {
    const fullModelId = "gpt-6-astra-202609";
    useModelStatsMock.mockReturnValue({
      data: [{ ...stats[0], model: fullModelId }],
      isLoading: false,
      isError: false,
      refetch: vi.fn(),
    });
    render(<ModelStatsTable range={{ preset: "7d" }} refreshIntervalMs={0} />);
    fireEvent.click(
      screen.getByRole("button", {
        name: "usage.modelDistribution.tokenShare / usage.modelDistribution.costShare",
      }),
    );

    const labels = Array.from(document.querySelectorAll("svg text"));
    expect(labels).toHaveLength(2);
    for (const label of labels) {
      expect(label.lastChild?.textContent).toBe(`${fullModelId} 100.0%`);
    }
    const legend = screen.getByRole("button", {
      name: /^gpt-6-astra-202609,/,
    });
    expect(within(legend).getByText(fullModelId)).toBeInTheDocument();
  });

  it("shares legend visibility across the charts and resets it on filter change", () => {
    const { rerender } = render(
      <ModelStatsTable range={{ preset: "today" }} refreshIntervalMs={30000} />,
    );
    const expand = screen.getByRole("button", {
      name: "usage.modelDistribution.tokenShare / usage.modelDistribution.costShare",
    });
    fireEvent.click(expand);
    expect(expand).toHaveAttribute("aria-expanded", "true");
    const alpha = screen.getByRole("button", { name: /^alpha,/ });
    expect(alpha).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(alpha);
    expect(alpha).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(expand);
    expect(expand).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(expand);
    expect(screen.getByRole("button", { name: /^alpha,/ })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(
      within(screen.getByRole("table")).getByText("alpha"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "usage.modelDistribution.showAll" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /^beta,/ }));
    expect(
      screen.getAllByText("usage.modelDistribution.allHidden"),
    ).toHaveLength(1);

    rerender(
      <ModelStatsTable range={{ preset: "7d" }} refreshIntervalMs={30000} />,
    );
    expect(screen.getByRole("button", { name: /^alpha,/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });
  it("shows a single empty state when no models match", () => {
    useModelStatsMock.mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
      refetch: vi.fn(),
    });
    render(<ModelStatsTable range={{ preset: "7d" }} refreshIntervalMs={0} />);
    expect(screen.getAllByText("usage.noData")).toHaveLength(1);
  });
});
