import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TrendSection } from "@/components/usage/TrendSection";

// Both charts are heavy (recharts + react-query); stub them down to a marker so
// this test only asserts the switch wiring.
vi.mock("@/components/usage/UsageTrendChart", () => ({
  UsageTrendChart: ({ titleSlot }: { titleSlot?: React.ReactNode }) => (
    <div data-testid="usage-chart">{titleSlot}</div>
  ),
}));
vi.mock("@/components/usage/QuotaTrendChart", () => ({
  QuotaTrendChart: ({ titleSlot }: { titleSlot?: React.ReactNode }) => (
    <div data-testid="quota-chart">{titleSlot}</div>
  ),
}));

const props = {
  range: { preset: "today" } as const,
  rangeLabel: "today",
  refreshIntervalMs: 0,
};

describe("TrendSection", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("shows the usage chart by default", () => {
    render(<TrendSection {...props} />);
    expect(screen.getByTestId("usage-chart")).toBeInTheDocument();
    expect(screen.queryByTestId("quota-chart")).toBeNull();
  });

  it("swaps to the quota chart when the switch is flipped", () => {
    render(<TrendSection {...props} />);
    fireEvent.click(screen.getByRole("switch"));
    expect(screen.getByTestId("quota-chart")).toBeInTheDocument();
    expect(screen.queryByTestId("usage-chart")).toBeNull();
  });

  it("switches back through the label buttons", () => {
    render(<TrendSection {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "额度" }));
    expect(screen.getByTestId("quota-chart")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "使用" }));
    expect(screen.getByTestId("usage-chart")).toBeInTheDocument();
  });

  it("remembers the choice across remounts", () => {
    const first = render(<TrendSection {...props} />);
    fireEvent.click(screen.getByRole("switch"));
    first.unmount();

    render(<TrendSection {...props} />);
    expect(screen.getByTestId("quota-chart")).toBeInTheDocument();
  });
});
