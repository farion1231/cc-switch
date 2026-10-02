import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderStatsTable } from "@/components/usage/ProviderStatsTable";
import {
  getStatsEstimatedSpeed,
  getStatsSpeed,
} from "@/components/usage/statsColumns";
import type { ProviderStats } from "@/types/usage";

const useProviderStatsMock = vi.hoisted(() => vi.fn());
const useProxyStatusQueryMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useProviderStats: (...args: unknown[]) => useProviderStatsMock(...args),
}));

vi.mock("@/lib/query/proxy", () => ({
  useProxyStatusQuery: () => useProxyStatusQueryMock(),
}));

const stat = (overrides: Partial<ProviderStats>): ProviderStats => ({
  providerId: "p",
  providerName: "P",
  requestCount: 1,
  totalTokens: 1_000,
  totalCost: "1",
  successRate: 100,
  avgLatencyMs: 1_000,
  ...overrides,
});

/** 在飞列在成本之后、成功率/速度之前，所以是第 5 个单元格（下标 4）。 */
const inFlightCell = (row: HTMLElement | null | undefined) =>
  row?.cells[4]?.textContent;

describe("ProviderStatsTable", () => {
  it("computes speed as total output over total generation time", () => {
    expect(
      getStatsSpeed(
        stat({ speedOutputTokens: 1_200, speedGenerationMs: 10_500 }),
      ),
    ).toBe("114");
    expect(
      getStatsSpeed(stat({ speedOutputTokens: 0, speedGenerationMs: 0 })),
    ).toBeNull();
    // 老后端没有这两个字段
    expect(getStatsSpeed(stat({}))).toBeNull();
  });

  it("falls back to the estimated speed, marked with ≈, when there is no exact one", () => {
    expect(
      getStatsEstimatedSpeed(
        stat({ estSpeedOutputTokens: 2_500, estSpeedDurationMs: 25_000 }),
      ),
    ).toBe("100");
    expect(getStatsEstimatedSpeed(stat({}))).toBeNull();

    useProviderStatsMock.mockReturnValue({
      isLoading: false,
      data: [
        stat({
          providerId: "_session",
          providerName: "Claude session",
          requestCount: 9,
          estSpeedOutputTokens: 2_500,
          estSpeedDurationMs: 25_000,
        }),
        // 两种都有时用精确的，不带 ≈
        stat({
          providerId: "mixed",
          providerName: "Mixed",
          requestCount: 3,
          speedOutputTokens: 9_200,
          speedGenerationMs: 100_000,
          estSpeedOutputTokens: 2_500,
          estSpeedDurationMs: 25_000,
        }),
      ],
    });

    render(
      <ProviderStatsTable range={{ preset: "7d" }} refreshIntervalMs={0} />,
    );

    const rows = screen.getAllByRole("row").slice(1);
    expect(rows[0].lastElementChild).toHaveTextContent("≈100tok/s");
    expect(rows[1].lastElementChild).toHaveTextContent("92tok/s");
    expect(rows[1].lastElementChild).not.toHaveTextContent("≈");
  });

  it("sorts by requests and replaces average latency with speed", () => {
    useProviderStatsMock.mockReturnValue({
      isLoading: false,
      data: [
        stat({
          providerId: "a",
          providerName: "Kimi For Coding",
          requestCount: 3,
        }),
        stat({
          providerId: "b",
          providerName: "DeepSeek",
          requestCount: 9,
          speedOutputTokens: 9_200,
          speedGenerationMs: 100_000,
        }),
      ],
    });

    render(
      <ProviderStatsTable range={{ preset: "7d" }} refreshIntervalMs={0} />,
    );

    expect(screen.queryByText("usage.avgLatency")).not.toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: /usage.speed/ }),
    ).toBeInTheDocument();
    const rows = screen.getAllByRole("row").slice(1);
    expect(rows[0]).toHaveTextContent("DeepSeek");
    expect(rows[0].lastElementChild).toHaveTextContent("92tok/s");
    expect(rows[1]).toHaveTextContent("Kimi For Coding");
    expect(rows[1].lastElementChild).toHaveTextContent("—");
  });
});

describe("ProviderStatsTable in-flight column", () => {
  const renderTable = (
    overrides: {
      appType?: string;
      providerName?: string;
      model?: string;
    } = {},
  ) =>
    render(
      <ProviderStatsTable
        range={{ preset: "7d" }}
        refreshIntervalMs={0}
        {...overrides}
      />,
    );

  it("renders the live in-flight count reported for the provider", () => {
    useProviderStatsMock.mockReturnValue({
      data: [stat({ providerId: "p1", providerName: "Provider One" })],
      isLoading: false,
    });
    useProxyStatusQueryMock.mockReturnValue({
      data: { in_flight_by_provider: { p1: 3 } },
    });

    renderTable();

    expect(screen.getByText("usage.inFlight")).toBeTruthy();
    // Cell by cell so a value can never be "found" inside a neighbouring one
    // (e.g. "1" is a substring of "1,000").
    const row = screen.getByText("Provider One").closest("tr");
    expect(inFlightCell(row)).toBe("3");
    expect(row?.cells[1]?.textContent).toBe("1");
    expect(row?.cells[2]?.textContent).toBe("1,000");
    expect(row?.cells[3]?.textContent).toBe("$1.00");
  });

  it("falls back to zero when the provider has no in-flight entry", () => {
    useProviderStatsMock.mockReturnValue({
      data: [stat({ providerId: "p1", providerName: "Provider One" })],
      isLoading: false,
    });
    useProxyStatusQueryMock.mockReturnValue({
      data: { in_flight_by_provider: {} },
    });

    renderTable();

    // Only one numeric cell is "0": the missing in-flight entry.
    const row = screen.getByText("Provider One").closest("tr");
    expect(inFlightCell(row)).toBe("0");
  });

  it("still renders when the proxy status has not loaded yet", () => {
    useProviderStatsMock.mockReturnValue({
      data: [stat({ providerId: "p1", providerName: "Provider One" })],
      isLoading: false,
    });
    useProxyStatusQueryMock.mockReturnValue({ data: undefined });

    renderTable();

    expect(screen.getByText("Provider One")).toBeTruthy();
    const row = screen.getByText("Provider One").closest("tr");
    expect(inFlightCell(row)).toBe("0");
  });

  it("spans every column in the empty state", () => {
    useProviderStatsMock.mockReturnValue({ data: [], isLoading: false });
    useProxyStatusQueryMock.mockReturnValue({ data: undefined });

    renderTable();

    const cell = screen.getByText("usage.noData");
    expect(cell.getAttribute("colspan")).toBe("7");
  });

  it("marks the count as provider-wide once a model filter is active", () => {
    // The backend counts per provider, not per model, so with a model filter on
    // the number no longer describes the rows beside it.
    useProviderStatsMock.mockReturnValue({
      data: [stat({ providerId: "p1", providerName: "Provider One" })],
      isLoading: false,
    });
    useProxyStatusQueryMock.mockReturnValue({
      data: { in_flight_by_provider: { p1: 3 } },
    });

    renderTable({ model: "claude-sonnet-4-6" });

    expect(screen.getByTitle("usage.inFlightUnfilteredHint")).toBeTruthy();
  });

  it("leaves the count unmarked when no model filter is active", () => {
    useProviderStatsMock.mockReturnValue({
      data: [stat({ providerId: "p1", providerName: "Provider One" })],
      isLoading: false,
    });
    useProxyStatusQueryMock.mockReturnValue({
      data: { in_flight_by_provider: { p1: 3 } },
    });

    renderTable();

    expect(screen.queryByTitle("usage.inFlightUnfilteredHint")).toBeNull();
  });
});
