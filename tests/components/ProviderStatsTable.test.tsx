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
  appType: "claude",
  providerName: "Provider One",
  requestCount: 12,
  totalTokens: 3456,
  totalCost: "0.0123",
  successRate: 100,
  avgLatencyMs: 420,
};

const renderTable = (
  overrides: {
    appType?: string;
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

describe("ProviderStatsTable", () => {
  it("renders recorded usage with the live in-flight count and app label", () => {
    useProviderStatsMock.mockReturnValue({ data: [stat], isLoading: false });
    useProxyStatusQueryMock.mockReturnValue({
      data: { in_flight_by_provider: { claude: { p1: 3 } } },
    });

    renderTable();

    const row = screen.getByText("Provider One").closest("tr");
    // The count is keyed by (app_type, provider_id) — the live state rides on
    // the recorded row instead of fabricating a new one.
    expect(row?.cells[6]?.textContent).toBe("3");
    expect(row?.textContent).toContain("Claude");
    // Cell by cell so a value can never be "found" inside a neighbouring one
    // (e.g. "12" is a substring of the cost "$0.0123").
    expect(row?.cells[1]?.textContent).toBe("12");
    expect(row?.cells[2]?.textContent).toBe("3,456");
    expect(row?.cells[3]?.textContent).toBe("$0.0123");
    expect(row?.cells[4]?.textContent).toBe("100.0%");
    expect(row?.cells[5]?.textContent).toBe("420ms");
  });

  it("falls back to zero when the provider has no in-flight entry", () => {
    useProviderStatsMock.mockReturnValue({ data: [stat], isLoading: false });
    useProxyStatusQueryMock.mockReturnValue({
      data: { in_flight_by_provider: {} },
    });

    renderTable();

    expect(
      screen.getByText("Provider One").closest("tr")?.cells[6]?.textContent,
    ).toBe("0");
  });

  it("still renders when the proxy status has not loaded yet", () => {
    useProviderStatsMock.mockReturnValue({ data: [stat], isLoading: false });
    useProxyStatusQueryMock.mockReturnValue({ data: undefined });

    renderTable();

    const row = screen.getByText("Provider One").closest("tr");
    expect(row?.cells[6]?.textContent).toBe("0");
  });

  it("keeps providers that share an id across apps on separate rows and counts", () => {
    // providers' primary key is (id, app_type), so importing every app yields a
    // "default" provider under each; the stats SQL groups by (provider_id,
    // app_type) and returns both rows. The in-flight lookup follows the same
    // composite identity: only Claude's row may borrow Claude's live count.
    useProviderStatsMock.mockReturnValue({
      data: [
        stat,
        {
          ...stat,
          appType: "codex",
          providerName: "Provider One",
          requestCount: 7,
          totalTokens: 111,
        },
      ],
      isLoading: false,
    });
    useProxyStatusQueryMock.mockReturnValue({
      data: { in_flight_by_provider: { claude: { p1: 3 } } },
    });

    renderTable();

    const rows = screen
      .getAllByText("Provider One")
      .map((el) => el.closest("tr"));
    expect(rows).toHaveLength(2);
    expect(rows.map((row) => row?.textContent)).toContainEqual(
      expect.stringContaining("Claude"),
    );
    expect(rows.map((row) => row?.textContent)).toContainEqual(
      expect.stringContaining("Codex"),
    );
    const byRequests = new Map(
      rows.map((row) => [
        row?.cells[1]?.textContent,
        row?.cells[6]?.textContent,
      ]),
    );
    expect(byRequests.get("12")).toBe("3");
    expect(byRequests.get("7")).toBe("0");
  });

  it("shows the honest empty state even while streams are in flight", () => {
    // A provider whose first stream of the day has not finished has no recorded
    // usage, so it gets no row: every column of a fabricated row would lie
    // about the selected range (0 requests / 0 tokens / $0). The live state is
    // the proxy panel's job, not a stats row.
    useProviderStatsMock.mockReturnValue({ data: [], isLoading: false });
    useProxyStatusQueryMock.mockReturnValue({
      data: { in_flight_by_provider: { claude: { "fresh-provider": 2 } } },
    });

    renderTable();

    const cell = screen.getByText("暂无数据");
    expect(cell.getAttribute("colspan")).toBe("7");
    expect(screen.queryByText("fresh-provider")).toBeNull();
  });

  it("marks the count as provider-wide once a model filter is active", () => {
    // The backend counts per provider, not per model, so with a model filter on
    // the number no longer describes the rows beside it.
    useProviderStatsMock.mockReturnValue({ data: [stat], isLoading: false });
    useProxyStatusQueryMock.mockReturnValue({
      data: { in_flight_by_provider: { claude: { p1: 3 } } },
    });

    renderTable({ model: "claude-sonnet-4-6" });

    expect(screen.getByTitle("usage.inFlightUnfilteredHint")).toBeTruthy();
  });

  it("leaves the count unmarked when no model filter is active", () => {
    useProviderStatsMock.mockReturnValue({ data: [stat], isLoading: false });
    useProxyStatusQueryMock.mockReturnValue({
      data: { in_flight_by_provider: { claude: { p1: 3 } } },
    });

    renderTable();

    expect(screen.queryByTitle("usage.inFlightUnfilteredHint")).toBeNull();
  });

  it("labels folded app types on the row", () => {
    // claude-desktop rows keep their raw app_type in the row projection even
    // though the app filter folds them into "claude"; the badge still says
    // where the numbers came from instead of masquerading as plain "claude".
    useProviderStatsMock.mockReturnValue({
      data: [{ ...stat, appType: "claude-desktop" }],
      isLoading: false,
    });
    useProxyStatusQueryMock.mockReturnValue({ data: undefined });

    renderTable();

    expect(screen.getByText("Claude Desktop")).toBeTruthy();
  });
});
