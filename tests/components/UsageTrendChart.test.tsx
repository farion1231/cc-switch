import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageTrendChart } from "@/components/usage/UsageTrendChart";

const useUsageTrendsMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, fallback?: string) => fallback ?? _key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useUsageTrends: (...args: unknown[]) => useUsageTrendsMock(...args),
}));

vi.mock("recharts", () => {
  const Container = ({ children }: { children?: ReactNode }) => (
    <div>{children}</div>
  );
  return {
    AreaChart: Container,
    LineChart: Container,
    ResponsiveContainer: Container,
    Area: () => null,
    Line: () => null,
    CartesianGrid: () => null,
    Legend: () => null,
    Tooltip: () => null,
    YAxis: () => null,
    Brush: () => null,
    XAxis: ({ domain }: { domain?: [number, number] }) => (
      <div data-testid="trend-x-axis" data-domain={JSON.stringify(domain)} />
    ),
  };
});

const response = {
  data: [
    {
      bucketStart: 1_000,
      bucketSeconds: 60,
      requestCount: 1,
      totalCost: "0.010000",
      totalTokens: 15,
      totalInputTokens: 10,
      totalOutputTokens: 5,
      totalCacheCreationTokens: 0,
      totalCacheReadTokens: 0,
    },
    {
      bucketStart: 2_000,
      bucketSeconds: 60,
      requestCount: 2,
      totalCost: "0.020000",
      totalTokens: 30,
      totalInputTokens: 20,
      totalOutputTokens: 10,
      totalCacheCreationTokens: 0,
      totalCacheReadTokens: 0,
    },
  ],
  granularity: { value: 1, unit: "minute" as const },
  precision: "daily" as const,
  detailCutoff: 500,
};

const renderChart = () =>
  render(
    <UsageTrendChart
      range={{ preset: "today" }}
      rangeLabel="Today"
      refreshIntervalMs={0}
    />,
  );

// WebKit's GestureEvent isn't in jsdom; synthesize one carrying the `scale` the
// chart reads. Dispatching on a chart-internal element makes the handler's
// inChart check (target contained by the chart node) pass.
const dispatchGesture = (
  target: Element,
  type: string,
  { scale, clientX = 100 }: { scale: number; clientX?: number },
) => {
  const ev = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(ev, "scale", { value: scale });
  Object.defineProperty(ev, "clientX", { value: clientX });
  // Wrap in act: the handler calls setVisibleDomain synchronously.
  act(() => {
    target.dispatchEvent(ev);
  });
};

describe("UsageTrendChart", () => {
  beforeEach(() => {
    useUsageTrendsMock.mockReset();
    useUsageTrendsMock.mockReturnValue({
      data: response,
      isLoading: false,
      isFetching: false,
    });
  });

  it("requests automatic granularity and explains daily-only history", () => {
    renderChart();

    expect(useUsageTrendsMock).toHaveBeenCalledWith(
      { preset: "today" },
      { appType: undefined, providerName: undefined, model: undefined },
      { mode: "auto", targetPoints: 500, maxPoints: 2000 },
      { refetchInterval: false },
    );
    expect(
      screen.getByText(
        "Older history only retains calendar-day summaries; sub-day detail cannot be restored.",
      ),
    ).toBeInTheDocument();
  });

  it("zooms the numeric time domain from the keyboard", async () => {
    renderChart();
    const chart = screen.getByRole("application");
    const initial = screen.getAllByTestId("trend-x-axis")[0].dataset.domain;

    fireEvent.keyDown(chart, { key: "+" });

    await waitFor(() =>
      expect(screen.getAllByTestId("trend-x-axis")[0].dataset.domain).not.toBe(
        initial,
      ),
    );
  });

  it("zooms in on trackpad pinch-out (gesturechange scale>1)", async () => {
    renderChart();
    const chart = screen.getByRole("application");
    const axis = () => screen.getAllByTestId("trend-x-axis")[0];
    const initial = JSON.parse(axis().dataset.domain ?? "[]") as number[];
    const initialSpan = initial[1] - initial[0];

    dispatchGesture(chart, "gesturestart", { scale: 1 });
    dispatchGesture(chart, "gesturechange", { scale: 1.5 });

    // scale>1 (fingers apart) must zoom IN -> visible span shrinks.
    await waitFor(() => {
      const next = JSON.parse(axis().dataset.domain ?? "[]") as number[];
      expect(next[1] - next[0]).toBeLessThan(initialSpan);
    });
  });

  it("swallows ctrl+wheel while a trackpad gesture is active", async () => {
    renderChart();
    const chart = screen.getByRole("application");
    const axis = () => screen.getAllByTestId("trend-x-axis")[0];
    const initial = JSON.parse(axis().dataset.domain ?? "[]") as number[];
    const fullSpan = initial[1] - initial[0];

    // Pinch in first so we have a zoomed domain that a stray wheel could disturb.
    dispatchGesture(chart, "gesturestart", { scale: 1 });
    dispatchGesture(chart, "gesturechange", { scale: 1.5 });
    await waitFor(() => {
      const d = JSON.parse(axis().dataset.domain ?? "[]") as number[];
      expect(d[1] - d[0]).toBeLessThan(fullSpan);
    });
    const zoomed = axis().dataset.domain;

    // While the gesture owns the input, the spurious ctrl+wheel must NOT zoom.
    chart.dispatchEvent(
      new WheelEvent("wheel", {
        ctrlKey: true,
        deltaY: -1000,
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(axis().dataset.domain).toBe(zoomed);
  });

  it("extends the viewport to new buckets on refresh when not zoomed", async () => {
    const { rerender } = renderChart();
    const axis = () => screen.getAllByTestId("trend-x-axis")[0];
    await waitFor(() =>
      expect(JSON.parse(axis().dataset.domain ?? "[]")[1]).toBe(2_060_000),
    );

    // Simulate a live refresh appending a new bucket at 3000s.
    useUsageTrendsMock.mockReturnValue({
      data: {
        data: [
          ...response.data,
          {
            bucketStart: 3_000,
            bucketSeconds: 60,
            requestCount: 1,
            totalCost: "0.000000",
            totalTokens: 0,
            totalInputTokens: 0,
            totalOutputTokens: 0,
            totalCacheCreationTokens: 0,
            totalCacheReadTokens: 0,
          },
        ],
        granularity: response.granularity,
        precision: response.precision,
        detailCutoff: response.detailCutoff,
      },
      isLoading: false,
      isFetching: false,
    });
    rerender(
      <UsageTrendChart
        range={{ preset: "today" }}
        rangeLabel="Today"
        refreshIntervalMs={0}
      />,
    );

    // Viewport was at the full range, so it should follow the new end (3_060_000)
    // instead of staying clipped at the old end (2_060_000).
    await waitFor(() =>
      expect(JSON.parse(axis().dataset.domain ?? "[]")[1]).toBe(3_060_000),
    );
  });
});
