import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderStatsTable } from "@/components/usage/ProviderStatsTable";
import type { ProviderStats } from "@/types/usage";

const useProviderStatsMock = vi.hoisted(() => vi.fn());
const useProxyStatusQueryMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    // The component passes the fallback as the second positional argument.
    t: (key: string, defaultValue?: string) => defaultValue ?? key,
    i18n: {
      resolvedLanguage: "en",
      language: "en",
    },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useProviderStats: (...args: unknown[]) => useProviderStatsMock(...args),
}));

vi.mock("@/lib/query/proxy", () => ({
  useProxyStatusQuery: () => useProxyStatusQueryMock(),
}));

const stat: ProviderStats = {
  providerId: "p1",
  providerName: "Provider One",
  requestCount: 12,
  totalTokens: 3456,
  totalCost: "0.0123",
  successRate: 100,
  avgLatencyMs: 420,
};

const renderTable = (overrides: { model?: string } = {}) =>
  render(
    <ProviderStatsTable
      range={{ preset: "7d" }}
      refreshIntervalMs={0}
      {...overrides}
    />,
  );

describe("ProviderStatsTable in-flight column", () => {
  it("renders the live in-flight count reported for the provider", () => {
    useProviderStatsMock.mockReturnValue({ data: [stat], isLoading: false });
    useProxyStatusQueryMock.mockReturnValue({
      data: { in_flight_by_provider: { p1: 3 } },
    });

    renderTable();

    expect(screen.getByText("推理流")).toBeTruthy();
    expect(screen.getByText("3")).toBeTruthy();
  });

  it("falls back to zero when the provider has no in-flight entry", () => {
    useProviderStatsMock.mockReturnValue({ data: [stat], isLoading: false });
    useProxyStatusQueryMock.mockReturnValue({
      data: { in_flight_by_provider: {} },
    });

    renderTable();

    // Only one numeric cell is "0": the missing in-flight entry.
    expect(screen.getByText("0")).toBeTruthy();
  });

  it("still renders when the proxy status has not loaded yet", () => {
    useProviderStatsMock.mockReturnValue({ data: [stat], isLoading: false });
    useProxyStatusQueryMock.mockReturnValue({ data: undefined });

    renderTable();

    expect(screen.getByText("Provider One")).toBeTruthy();
    expect(screen.getByText("0")).toBeTruthy();
  });

  it("spans every column in the empty state", () => {
    useProviderStatsMock.mockReturnValue({ data: [], isLoading: false });
    useProxyStatusQueryMock.mockReturnValue({ data: undefined });

    renderTable();

    const cell = screen.getByText("暂无数据");
    expect(cell.getAttribute("colspan")).toBe("7");
  });

  it("marks the count as provider-wide once a model filter is active", () => {
    // The backend counts per provider, not per model, so with a model filter on
    // the number no longer describes the rows beside it.
    useProviderStatsMock.mockReturnValue({ data: [stat], isLoading: false });
    useProxyStatusQueryMock.mockReturnValue({
      data: { in_flight_by_provider: { p1: 3 } },
    });

    renderTable({ model: "claude-sonnet-4-6" });

    expect(screen.getByTitle("usage.inFlightUnfilteredHint")).toBeTruthy();
  });

  it("leaves the count unmarked when no model filter is active", () => {
    useProviderStatsMock.mockReturnValue({ data: [stat], isLoading: false });
    useProxyStatusQueryMock.mockReturnValue({
      data: { in_flight_by_provider: { p1: 3 } },
    });

    renderTable();

    expect(screen.queryByTitle("usage.inFlightUnfilteredHint")).toBeNull();
  });
});
