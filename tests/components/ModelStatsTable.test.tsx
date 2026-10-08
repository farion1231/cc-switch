import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ModelStatsTable } from "@/components/usage/ModelStatsTable";

const useModelStatsMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/query/usage", () => ({
  useModelStats: (...args: unknown[]) => useModelStatsMock(...args),
}));

const stats = [
  {
    model: "alpha",
    requestCount: 2,
    totalTokens: 900,
    totalCost: "0.40",
    avgCostPerRequest: "0.20",
    successRate: 100,
  },
  {
    model: "beta",
    requestCount: 5,
    totalTokens: 100,
    totalCost: "0.10",
    avgCostPerRequest: "0.02",
    successRate: 100,
  },
];

describe("ModelStatsTable", () => {
  beforeEach(() => {
    useModelStatsMock.mockReturnValue({ data: stats, isLoading: false });
  });

  it("defaults to total cost descending", () => {
    render(
      <ModelStatsTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );

    const rows = screen.getAllByRole("row");
    expect(within(rows[1]).getByText("alpha")).toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: "usage.cost" }),
    ).toHaveAttribute("aria-sort", "descending");
  });

  it("sorts every numeric metric in both directions", () => {
    render(
      <ModelStatsTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "usage.requests" }));
    let rows = screen.getAllByRole("row");
    expect(within(rows[1]).getByText("beta")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "usage.requests" }));
    rows = screen.getAllByRole("row");
    expect(within(rows[1]).getByText("alpha")).toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: "usage.requests" }),
    ).toHaveAttribute("aria-sort", "ascending");

    fireEvent.click(screen.getByRole("button", { name: "usage.tokens" }));
    rows = screen.getAllByRole("row");
    expect(within(rows[1]).getByText("alpha")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "usage.tokens" }));
    rows = screen.getAllByRole("row");
    expect(within(rows[1]).getByText("beta")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "usage.cost" }));
    rows = screen.getAllByRole("row");
    expect(within(rows[1]).getByText("alpha")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "usage.cost" }));
    rows = screen.getAllByRole("row");
    expect(within(rows[1]).getByText("beta")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "usage.avgCost" }));
    rows = screen.getAllByRole("row");
    expect(within(rows[1]).getByText("alpha")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "usage.avgCost" }));
    rows = screen.getAllByRole("row");
    expect(within(rows[1]).getByText("beta")).toBeInTheDocument();
  });
});
