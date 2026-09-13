import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  formatHeatmapBucketRange,
  getHeatmapIntensityLevel,
  UsageHeatmap,
} from "@/components/usage/UsageHeatmap";
import { usageKeys } from "@/lib/query/usage";
import type { UsageHeatmapPoint, UsageRangeSelection } from "@/types/usage";

const useUsageHeatmapMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/query/usage", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/query/usage")>(
      "@/lib/query/usage",
    );
  return {
    ...actual,
    useUsageHeatmap: (...args: unknown[]) => useUsageHeatmapMock(...args),
  };
});

vi.mock("@/components/ui/select", () => ({
  Select: ({ value, onValueChange, children }: any) => (
    <select
      aria-label="metric-select"
      value={value}
      onChange={(event) => onValueChange(event.target.value)}
    >
      {children}
    </select>
  ),
  SelectTrigger: ({ children }: any) => <>{children}</>,
  SelectValue: () => null,
  SelectContent: ({ children }: any) => <>{children}</>,
  SelectItem: ({ value, children }: any) => (
    <option value={value}>{children}</option>
  ),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (key === "usage.heatmap.cellLabel") {
        return `${options?.range}, ${options?.metric}: ${options?.value}`;
      }
      if (key === "usage.heatmap.bucketMinutes") {
        return `bucket minutes: ${options?.minutes}`;
      }
      return key;
    },
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

const range: UsageRangeSelection = { preset: "1d" };
const start = Math.floor(new Date(2026, 6, 11, 3, 0).getTime() / 1000);

function makePoints(count: number): UsageHeatmapPoint[] {
  return Array.from({ length: count }, (_, index) => ({
    bucketStart: start + index * 15 * 60,
    bucketEnd: start + (index + 1) * 15 * 60,
    requestCount: index === 0 ? 10 : 5,
    successfulRequests: index === 0 ? 9 : 5,
    failedRequests: index === 0 ? 1 : 0,
    requestsWithUsage: index === 0 ? 8 : 5,
    totalTokens: index === 0 ? 100 : 1_000,
  }));
}

describe("UsageHeatmap helpers", () => {
  it("maps either metric to stable intensity levels", () => {
    expect(getHeatmapIntensityLevel(0, 10)).toBe(0);
    expect(getHeatmapIntensityLevel(Number.NaN, 10)).toBe(0);
    expect(getHeatmapIntensityLevel(1, 10)).toBe(1);
    expect(getHeatmapIntensityLevel(5, 10)).toBe(2);
    expect(getHeatmapIntensityLevel(10, 10)).toBe(4);
    expect(getHeatmapIntensityLevel(20, 10)).toBe(4);
  });

  it("formats same-day and cross-day bucket boundaries to the minute", () => {
    expect(formatHeatmapBucketRange(start, start + 15 * 60)).toBe(
      "2026-07-11 03:00 - 03:15",
    );
    const late = Math.floor(new Date(2026, 6, 11, 23, 45).getTime() / 1000);
    expect(formatHeatmapBucketRange(late, late + 30 * 60)).toBe(
      "2026-07-11 23:45 - 2026-07-12 00:15",
    );
  });

  it("includes the range and all scope filters in the query key", () => {
    expect(
      usageKeys.heatmap(
        "custom",
        100,
        200,
        {
          appType: "codex",
          providerName: "Provider A",
          model: "gpt-5",
        },
        true,
      ),
    ).toEqual([
      "usage",
      "heatmap",
      "custom",
      100,
      200,
      true,
      "codex",
      "Provider A",
      "gpt-5",
    ]);
  });
});

describe("UsageHeatmap", () => {
  const refetch = vi.fn();

  beforeEach(() => {
    refetch.mockReset();
    useUsageHeatmapMock.mockReset();
    useUsageHeatmapMock.mockReturnValue({
      data: {
        status: "available",
        bucketMinutes: 15,
        points: makePoints(10),
      },
      isLoading: false,
      isError: false,
      isFetching: false,
      refetch,
    });
  });

  it("renders dynamic buckets, portalled details, and refreshes", () => {
    render(
      <UsageHeatmap
        range={range}
        appType="all"
        providerName="Provider A"
        model="gpt-5"
        refreshIntervalMs={30_000}
      />,
    );

    const cells = screen.getAllByRole("gridcell");
    expect(cells).toHaveLength(10);
    expect(
      cells.filter((cell) => cell.getAttribute("tabindex") === "0"),
    ).toHaveLength(1);
    const scroll = screen.getByTestId("usage-heatmap-scroll");
    expect(scroll).toHaveClass("overflow-x-auto");

    fireEvent.click(cells[0]);
    const detailTitles = screen.getAllByText("2026-07-11 03:00 - 03:15");
    expect(detailTitles.length).toBeGreaterThan(0);
    for (const detailTitle of detailTitles) {
      expect(scroll).not.toContainElement(detailTitle);
      expect(document.body).toContainElement(detailTitle);
    }
    expect(screen.getAllByText("90.0%").length).toBeGreaterThan(0);

    fireEvent.click(
      screen.getByRole("button", { name: "usage.heatmap.refresh" }),
    );
    expect(refetch).toHaveBeenCalledTimes(1);
    expect(useUsageHeatmapMock).toHaveBeenCalledWith(
      range,
      {
        appType: "all",
        providerName: "Provider A",
        model: "gpt-5",
      },
      { refetchInterval: 30_000 },
    );
  });

  it("defaults to tokens and switches intensity to requests without refetching", () => {
    render(<UsageHeatmap range={range} refreshIntervalMs={0} />);
    const first = screen.getAllByRole("gridcell")[0];

    expect(screen.getByText("usage.heatmap.tokenTitle")).toBeInTheDocument();
    expect(first).toHaveClass("bg-heatmap-1");

    fireEvent.change(screen.getByLabelText("metric-select"), {
      target: { value: "requests" },
    });

    expect(screen.getByText("usage.heatmap.requestTitle")).toBeInTheDocument();
    expect(first).toHaveClass("bg-heatmap-4");
    expect(refetch).not.toHaveBeenCalled();
  });

  it("shows the current bucket length and updates it with the response", () => {
    const { rerender } = render(
      <UsageHeatmap range={range} refreshIntervalMs={0} />,
    );
    expect(screen.getByText("bucket minutes: 15")).toBeInTheDocument();

    useUsageHeatmapMock.mockReturnValue({
      data: {
        status: "available",
        bucketMinutes: 30,
        points: makePoints(5),
      },
      isLoading: false,
      isError: false,
      isFetching: false,
      refetch,
    });
    rerender(<UsageHeatmap range={range} refreshIntervalMs={0} />);

    expect(screen.getByText("bucket minutes: 30")).toBeInTheDocument();
    expect(screen.queryByText("bucket minutes: 15")).not.toBeInTheDocument();
  });

  it("updates details immediately when the pointer enters an adjacent cell", () => {
    render(<UsageHeatmap range={range} refreshIntervalMs={0} />);
    const cells = screen.getAllByRole("gridcell");

    fireEvent.pointerEnter(cells[0]);
    expect(cells[0]).toHaveAttribute("data-state", "instant-open");
    expect(
      screen.getAllByText("2026-07-11 03:00 - 03:15").length,
    ).toBeGreaterThan(0);

    fireEvent.pointerEnter(cells[1]);
    expect(cells[0]).toHaveAttribute("data-state", "closed");
    expect(cells[1]).toHaveAttribute("data-state", "instant-open");
    expect(
      screen.getAllByText("2026-07-11 03:15 - 03:30").length,
    ).toBeGreaterThan(0);
  });

  it("keeps arrow navigation inside an incomplete final column", () => {
    render(<UsageHeatmap range={range} refreshIntervalMs={0} />);
    const cells = screen.getAllByRole("gridcell");
    fireEvent.focus(cells[0]);

    fireEvent.keyDown(cells[0], { key: "ArrowRight" });
    expect(document.activeElement).toBe(cells[8]);
    fireEvent.keyDown(cells[8], { key: "ArrowDown" });
    expect(document.activeElement).toBe(cells[9]);
    fireEvent.keyDown(cells[9], { key: "ArrowDown" });
    expect(document.activeElement).toBe(cells[9]);
    fireEvent.keyDown(cells[9], { key: "ArrowRight" });
    expect(document.activeElement).toBe(cells[9]);
  });

  it("renders loading, error recovery, and detail-unavailable states", () => {
    useUsageHeatmapMock.mockReturnValueOnce({
      data: undefined,
      isLoading: true,
      isError: false,
      isFetching: true,
      refetch,
    });
    const { rerender } = render(
      <UsageHeatmap range={range} refreshIntervalMs={0} />,
    );
    expect(screen.getByLabelText("usage.heatmap.loading")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "usage.heatmap.refresh" }),
    ).toBeDisabled();

    useUsageHeatmapMock.mockReturnValueOnce({
      data: undefined,
      isLoading: false,
      isError: true,
      isFetching: false,
      refetch,
    });
    rerender(<UsageHeatmap range={range} refreshIntervalMs={0} />);
    fireEvent.click(
      screen.getByRole("button", { name: "usage.heatmap.retry" }),
    );
    expect(refetch).toHaveBeenCalledTimes(1);

    useUsageHeatmapMock.mockReturnValue({
      data: {
        status: "detailUnavailable",
        availableFrom: start,
        points: [],
      },
      isLoading: false,
      isError: false,
      isFetching: false,
      refetch,
    });
    rerender(<UsageHeatmap range={range} refreshIntervalMs={0} />);
    expect(
      screen.getByText("usage.heatmap.detailUnavailable"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "usage.heatmap.refresh" }),
    ).toBeDisabled();
  });
});
