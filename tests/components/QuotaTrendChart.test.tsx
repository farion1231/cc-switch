import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { QuotaTrendChart } from "@/components/usage/QuotaTrendChart";
import type { QuotaHistoryRow } from "@/lib/api/quotaHistory";

const query =
  vi.fn<
    (
      appId: string | null,
      startHour: number,
      endHour: number,
    ) => Promise<QuotaHistoryRow[]>
  >();

vi.mock("@/lib/api/quotaHistory", () => ({
  quotaHistoryApi: {
    query: (...args: [string | null, number, number]) => query(...args),
    record: vi.fn(),
  },
}));

function row(
  appId: string,
  hour: number,
  tier: string,
  utilization: number,
): QuotaHistoryRow {
  return { appId, hour, tier, utilization, usedUsd: null, maxUsd: null };
}

function renderChart(appType: string) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <QuotaTrendChart
        range={{ preset: "7d" }}
        rangeLabel="7d"
        appType={appType}
      />
    </QueryClientProvider>,
  );
}

describe("QuotaTrendChart", () => {
  beforeEach(() => {
    query.mockReset();
  });

  it("explains how history accumulates when there is none", async () => {
    query.mockResolvedValue([]);
    renderChart("claude");
    expect(await screen.findByText("该区间暂无额度记录")).toBeInTheDocument();
  });

  it("renders the chart once the probe has recorded something", async () => {
    const nowHour = Math.floor(Date.now() / 3_600_000);
    query.mockResolvedValue([
      row("claude", nowHour - 2, "five_hour", 30),
      row("claude", nowHour - 1, "five_hour", 55),
    ]);

    renderChart("claude");

    // jsdom gives ResponsiveContainer a 0x0 box, so recharts draws no lines —
    // the branch that matters here is that the chart replaced the empty state.
    // The series shape itself is covered in quotaHistory.test.ts.
    await vi.waitFor(() =>
      expect(
        document.querySelector(".recharts-responsive-container"),
      ).not.toBeNull(),
    );
    expect(screen.queryByText("该区间暂无额度记录")).toBeNull();
  });

  it("falls back to the first app with history when the filter is 'all'", async () => {
    const nowHour = Math.floor(Date.now() / 3_600_000);
    query.mockResolvedValue([row("codex", nowHour - 1, "five_hour", 30)]);

    renderChart("all");

    expect(await screen.findByText("codex")).toBeInTheDocument();
    expect(screen.queryByText("该区间暂无额度记录")).toBeNull();
  });

  it("queries the whole hour range for every app at once", async () => {
    query.mockResolvedValue([]);
    renderChart("claude");

    await vi.waitFor(() => expect(query).toHaveBeenCalled());
    const [appId, startHour, endHour] = query.mock.calls[0];
    expect(appId).toBeNull();
    expect(endHour - startHour).toBeGreaterThan(24 * 6);
  });
});
