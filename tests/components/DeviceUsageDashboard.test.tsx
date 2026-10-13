import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageDashboard } from "@/components/usage/UsageDashboard";
import { UsageDateRangePicker } from "@/components/usage/UsageDateRangePicker";
import { PerformanceCells } from "@/components/usage/statsColumns";
import { renderWithQueryClient } from "../utils/testQueryClient";

const api = vi.hoisted(() => ({
  status: vi.fn(),
  report: vi.fn(),
  configure: vi.fn(),
  sync: vi.fn(),
}));
const local = vi.hoisted(() => ({
  getUsageSummary: vi.fn(),
  getUsageSummaryByApp: vi.fn(),
  getUsageTrends: vi.fn(),
  getProviderStats: vi.fn(),
  getModelStats: vi.fn(),
  getRequestLogs: vi.fn(),
  getSessionUsageLastSync: vi.fn(),
  syncSessionUsage: vi.fn(),
  getUsageFirstDate: vi.fn(),
}));
const lang = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { resolvedLanguage: "en", language: "en" },
}));
vi.mock("@/lib/api/deviceUsage", () => ({ deviceUsageApi: api }));
vi.mock("@/lib/api/usage", () => ({ usageApi: local }));
vi.mock("react-i18next", () => ({ useTranslation: () => lang }));
vi.mock("@/hooks/useUsageEventBridge", () => ({
  useUsageEventBridge: () => {},
}));
vi.mock("@/lib/query/proxy", () => ({
  useGlobalProxyConfig: () => ({
    data: { enableLogging: true },
    isLoading: false,
  }),
}));
const date = new Date().toISOString().slice(0, 10);
const row = {
  date,
  appType: "codex",
  model: "fixture-proxy",
  verified: true,
  input: 50,
  output: 10,
  cacheRead: 30,
  cacheWrite: 0,
  cost: "0.1",
  requests: 1,
};
const session = {
  ...row,
  appType: "claude",
  model: "fixture-session",
  verified: false,
  input: 100,
  cost: "0.2",
  requests: 2,
};
const settings = {
  deviceId: "local",
  deviceName: "My Mac",
  enabled: true,
  lastSyncAt: null,
  lastError: null,
};
beforeEach(() => {
  vi.clearAllMocks();
  api.report.mockResolvedValue({
    settings,
    rows: [row, session],
    totals: [],
    devices: [
      {
        deviceId: "remote",
        deviceName: "Windows",
        local: false,
        capturedAt: 1,
      },
    ],
    conflictingEvents: 0,
  });
  api.status.mockResolvedValue({ settings, devices: [] });
  api.configure.mockResolvedValue(settings);
  api.sync.mockResolvedValue(undefined);
  local.getUsageSummary.mockResolvedValue({ totalRequests: 1 });
  local.getUsageSummaryByApp.mockResolvedValue([]);
  local.getUsageTrends.mockResolvedValue([]);
  local.getProviderStats.mockResolvedValue([]);
  local.getModelStats.mockResolvedValue([]);
  local.getSessionUsageLastSync.mockResolvedValue(null);
  local.getRequestLogs.mockResolvedValue({
    data: [],
    total: 0,
    page: 0,
    pageSize: 20,
  });
  local.getUsageFirstDate.mockResolvedValue(date);
});
async function selectAllDevices() {
  await waitFor(() => expect(api.status).toHaveBeenCalled());
  await userEvent.click(
    screen.getByRole("button", { name: "usage.devices.scope" }),
  );
  await userEvent.click(
    await screen.findByRole("menuitem", { name: "usage.devices.all" }),
  );
  await screen.findByRole("table", { name: "usage.devices.breakdown" });
}
describe("device usage in the existing dashboard", () => {
  it("loads only device status in the local view and keeps local usage available if status fails", async () => {
    api.status.mockRejectedValueOnce(new Error("synthetic status failure"));
    renderWithQueryClient(<UsageDashboard />);
    await waitFor(() => expect(api.status).toHaveBeenCalled());
    expect(api.report).not.toHaveBeenCalled();
    expect(local.getUsageSummaryByApp).toHaveBeenCalled();
    expect(local.getRequestLogs).toHaveBeenCalled();
    expect(
      await screen.findByRole("table", { name: "usage.requestLogs" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: /usage.presetToday/ }),
    );
    expect(
      screen.getByRole("button", { name: "usage.preset1d" }),
    ).toBeInTheDocument();
    expect(screen.getByText("usage.liveEndTime")).toBeInTheDocument();
    expect(document.querySelectorAll('input[type="time"]')).toHaveLength(2);
  });
  it("uses one header, application filter, calendar, hero, tabs and table for both scopes", async () => {
    renderWithQueryClient(<UsageDashboard />);
    await userEvent.click(
      screen.getByRole("button", { name: "usage.metrics.more" }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "usage.trend.cost" }),
    );
    await selectAllDevices();
    expect(
      screen.getByRole("button", { name: "usage.metrics.more" }),
    ).toHaveAttribute("aria-expanded", "true");
    expect(
      screen.getByRole("button", { name: "usage.trend.cost" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(screen.getAllByRole("heading", { name: "nav.usage" })).toHaveLength(
      1,
    );
    expect(
      screen.getAllByRole("group", { name: "usage.appFilter.label" }),
    ).toHaveLength(1);
    expect(
      screen.getAllByRole("button", { name: "usage.metrics.more" }),
    ).toHaveLength(1);
    expect(
      screen.getByRole("tab", { name: "usage.tabs.providers" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("tab", { name: "usage.tabs.pricing" }),
    ).toBeDisabled();
    expect(screen.getByText("usage.devices.estimatedCost")).toBeInTheDocument();
    expect(screen.getAllByText("$0.30").length).toBeGreaterThan(0);
    expect(
      screen.getByRole("button", { name: "usage.appFilter.all" }),
    ).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(screen.getByRole("button", { name: "Claude Code" }));
    await waitFor(() =>
      expect(
        screen.queryByRole("cell", { name: "fixture-proxy" }),
      ).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole("cell", { name: "fixture-session" }),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "usage.devices.scope" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "usage.devices.local" }),
    );
    expect(
      await screen.findByRole("tab", { name: "usage.requestLogs" }),
    ).not.toBeDisabled();
    expect(
      screen.getByRole("tab", { name: "usage.tabs.providers" }),
    ).not.toBeDisabled();
  });
  it("filters provenance through the existing table toolbar and displays unavailable response metrics honestly", async () => {
    renderWithQueryClient(<UsageDashboard />);
    await selectAllDevices();
    await userEvent.click(
      within(
        screen.getByRole("region", { name: "usage.tabs.label" }),
      ).getByRole("button", { name: "usage.devices.quality" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "usage.devices.verified" }),
    );
    expect(
      screen.queryByRole("cell", { name: "fixture-session" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("cell", { name: "fixture-proxy" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("usage.devices.estimatedCost"),
    ).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("tab", { name: "usage.tabs.models" }),
    );
    const table = await screen.findByRole("table", {
      name: "usage.modelStats",
    });
    expect(
      within(table).getByRole("columnheader", { name: "usage.successRate" }),
    ).toBeInTheDocument();
    expect(within(table).getAllByRole("cell", { name: "—" })).toHaveLength(3);
    expect(
      screen.queryByRole("cell", { name: "0.0%" }),
    ).not.toBeInTheDocument();
  });
  it("preserves upstream first-token statistics in the local view", async () => {
    local.getModelStats.mockResolvedValue([
      {
        model: "local-first-token",
        requestCount: 1,
        totalTokens: 100,
        totalCost: "0.1",
        avgCostPerRequest: "0.1",
        successRate: 100,
        firstTokenMedianMs: 1200,
        estFirstTokenMedianMs: 3300,
      },
    ]);
    renderWithQueryClient(<UsageDashboard />);
    await userEvent.click(
      screen.getByRole("tab", { name: "usage.tabs.models" }),
    );
    const table = await screen.findByRole("table", {
      name: "usage.modelStats",
    });
    expect(
      within(table).getByRole("columnheader", { name: /usage.firstToken/ }),
    ).toBeInTheDocument();
    const firstToken = await within(table).findByRole("cell", { name: "1.2s" });
    expect(firstToken).not.toHaveTextContent("≈");
    expect(within(table).getAllByRole("columnheader")).toHaveLength(7);
  });
  it("keeps sync settings inside the original data-sources sheet", async () => {
    renderWithQueryClient(<UsageDashboard />);
    await selectAllDevices();
    await userEvent.click(
      screen.getByRole("button", { name: "usage.dataSources" }),
    );
    const sheet = screen.getByRole("dialog", { name: "usage.dataSources" });
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    expect(
      within(sheet).getByRole("heading", { name: "usage.sources.scanTitle" }),
    ).toBeInTheDocument();
    const name = within(sheet).getByRole("textbox", {
      name: "usage.devices.name",
    });
    await userEvent.clear(name);
    await userEvent.type(name, "Mac mini");
    await userEvent.click(
      within(sheet).getByRole("switch", { name: "usage.devices.enable" }),
    );
    await userEvent.click(
      within(sheet).getByRole("button", { name: "common.save" }),
    );
    await waitFor(() =>
      expect(api.configure).toHaveBeenCalledWith(false, "Mac mini"),
    );
    await userEvent.click(
      within(sheet).getByRole("button", { name: "usage.devices.sync" }),
    );
    await waitFor(() => expect(api.sync).toHaveBeenCalledTimes(1));
    expect(local.syncSessionUsage).not.toHaveBeenCalled();
  });
  it("reuses the original calendar in UTC day mode and the heatmap for all-time usage", async () => {
    renderWithQueryClient(<UsageDashboard />);
    await selectAllDevices();
    await userEvent.click(
      screen.getByRole("button", { name: /usage.presetToday/ }),
    );
    expect(screen.getByText("usage.devices.dateHint")).toBeInTheDocument();
    expect(screen.queryByText("usage.liveEndTime")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "usage.preset1d" }),
    ).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "usage.presetAll" }),
    );
    expect(
      await screen.findByRole("heading", { name: "usage.heatmap.title" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("table", { name: "usage.devices.breakdown" }),
    ).toBeInTheDocument();
  });
});
describe("UTC calendar extension", () => {
  it("submits complete UTC days while keeping the original calendar and confirm flow", async () => {
    const onApply = vi.fn();
    renderWithQueryClient(
      <UsageDateRangePicker
        granularity="utc-day"
        selection={{
          preset: "custom",
          customStartDate: Date.parse("2026-01-01T00:00:00Z") / 1000,
          customEndDate: Date.parse("2026-01-02T23:59:59Z") / 1000,
        }}
        triggerLabel="range"
        onApply={onApply}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: /range/ }));
    await userEvent.click(
      screen.getByRole("button", { name: "common.confirm" }),
    );
    expect(onApply).toHaveBeenCalledWith({
      preset: "custom",
      customStartDate: Date.parse("2026-01-01T00:00:00Z") / 1000,
      customEndDate: Date.parse("2026-01-02T23:59:59Z") / 1000,
      liveEndTime: false,
    });
  });
});

describe("shared statistics compatibility", () => {
  it.each([
    [0, "0.0%"],
    [86.25, "86.3%"],
    [99.95, "100%"],
    [100, "100%"],
    [undefined, "—"],
  ] as const)(
    "preserves success-rate formatting for %s",
    (successRate, expected) => {
      renderWithQueryClient(
        <table>
          <tbody>
            <tr>
              <PerformanceCells stat={{ successRate }} />
            </tr>
          </tbody>
        </table>,
      );
      expect(screen.getAllByRole("cell")[0]).toHaveTextContent(expected);
    },
  );
});
