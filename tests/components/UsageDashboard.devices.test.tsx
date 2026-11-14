import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  UsageDashboard,
  resetUsageSyncClockForTests,
} from "@/components/usage/UsageDashboard";
import type { UsageSyncOverview } from "@/types/usageSync";

const useProviderStatsMock = vi.hoisted(() => vi.fn());
const useModelStatsMock = vi.hoisted(() => vi.fn());
const usageHeroMock = vi.hoisted(() => vi.fn());
const requestLogTableMock = vi.hoisted(() => vi.fn());
const overviewMock = vi.hoisted(() => vi.fn());
const usageApiMock = vi.hoisted(() => ({
  getUsageSummary: vi.fn(),
  syncSessionUsage: vi.fn(),
  getSessionUsageLastSync: vi.fn(),
  rebuildCodexUsage: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: unknown) => {
      if (options && typeof options === "object") {
        const values = Object.entries(options as Record<string, unknown>)
          .filter(([name]) => name !== "defaultValue")
          .map(([, value]) => String(value));
        return values.length ? `${key}:${values.join(",")}` : key;
      }
      return key;
    },
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), warning: vi.fn(), error: vi.fn() },
}));
vi.mock("@/hooks/useUsageEventBridge", () => ({
  useUsageEventBridge: () => {},
}));
vi.mock("@/lib/api/usage", () => ({ usageApi: usageApiMock }));
vi.mock("@/lib/query/usage", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/query/usage")>(
      "@/lib/query/usage",
    );
  return {
    ...actual,
    useProviderStats: (...args: unknown[]) => useProviderStatsMock(...args),
    useModelStats: (...args: unknown[]) => useModelStatsMock(...args),
  };
});
vi.mock("@/lib/query/usageSync", async () => {
  const actual = await vi.importActual<typeof import("@/lib/query/usageSync")>(
    "@/lib/query/usageSync",
  );
  return {
    ...actual,
    useUsageSyncOverview: () => overviewMock(),
    useUsageSyncEventBridge: () => {},
    useRunUsageSync: () => ({ mutateAsync: vi.fn(), isPending: false }),
    useSetUsageSyncDeviceHidden: () => ({ mutate: vi.fn() }),
  };
});
vi.mock("@/components/usage/UsageHero", () => ({
  UsageHero: (props: unknown) => {
    usageHeroMock(props);
    return <div data-testid="usage-hero" />;
  },
}));
vi.mock("@/components/usage/UsageTrendChart", () => ({
  UsageTrendChart: () => <div data-testid="usage-trend" />,
}));
vi.mock("@/components/usage/UsageHeatmap", () => ({
  UsageHeatmap: () => <div data-testid="usage-heatmap" />,
}));
vi.mock("@/components/usage/RequestLogTable", () => ({
  RequestLogTable: (props: unknown) => {
    requestLogTableMock(props);
    return <div data-testid="request-logs" />;
  },
}));
vi.mock("@/components/usage/RequestDetailPanel", () => ({
  RequestDetailPanel: () => null,
}));
vi.mock("@/components/usage/UsageDateRangePicker", () => ({
  UsageDateRangePicker: () => <button type="button">range</button>,
}));
vi.mock("@/lib/query/proxy", () => ({
  useGlobalProxyConfig: () => ({ data: { enableLogging: true } }),
}));

const NOW_SECS = Math.floor(Date.now() / 1000);

function overview(partial: Partial<UsageSyncOverview> = {}): UsageSyncOverview {
  return {
    settings: {
      enabled: true,
      autoSync: true,
      backend: "webdav",
      detailLevel: "records",
      intervalMinutes: 15,
      status: { identityConflict: false },
    },
    availableBackends: { webdav: true, s3: false },
    remoteLocation: "WebDAV: /cc-switch-sync/usage-v1/default",
    transportSecure: true,
    thisDevice: {
      deviceId: "local-id",
      deviceName: "MacBook",
      utcOffsetMinutes: 0,
      unverified: { requests: 0 },
    },
    devices: [
      {
        deviceId: "11111111-aaaa",
        deviceName: "Windows PC",
        detailLevel: "records",
        lastUploadAt: NOW_SECS - 60,
        lastFetchedAt: NOW_SECS - 30,
        status: "ok",
        logRows: 10,
        rollupRows: 0,
        unverified: { requests: 0 },
      },
      {
        deviceId: "22222222-bbbb",
        deviceName: "NAS",
        detailLevel: "daily",
        lastUploadAt: NOW_SECS - 3 * 86_400,
        lastFetchedAt: NOW_SECS - 30,
        status: "ok",
        logRows: 0,
        rollupRows: 4,
        unverified: { requests: 4 },
      },
    ],
    syncing: false,
    staleAfterSecs: 86_400,
    ...partial,
  };
}

const renderDashboard = () =>
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <UsageDashboard />
    </QueryClientProvider>,
  );

describe("UsageDashboard device scope", () => {
  beforeEach(() => {
    resetUsageSyncClockForTests();
    useProviderStatsMock.mockReset().mockReturnValue({ data: [] });
    useModelStatsMock.mockReset().mockReturnValue({ data: [] });
    usageHeroMock.mockReset();
    requestLogTableMock.mockReset();
    overviewMock.mockReset().mockReturnValue({ data: overview() });
    usageApiMock.getUsageSummary.mockResolvedValue({ totalRequests: 5 });
    usageApiMock.getSessionUsageLastSync.mockResolvedValue(null);
  });

  it("hides the device filter when usage sync is off and no devices exist", () => {
    overviewMock.mockReturnValue({
      data: overview({
        settings: {
          ...overview().settings,
          enabled: false,
        },
        devices: [],
      }),
    });
    renderDashboard();
    expect(
      screen.queryByRole("button", { name: /usageSync.deviceFilter.title/ }),
    ).not.toBeInTheDocument();
    // Local scope keeps the historical call shape: no device value at all.
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ device: undefined }),
    );
  });

  it("defaults to this device and switches every query to all devices", async () => {
    const user = userEvent.setup();
    renderDashboard();

    const trigger = screen.getByRole("button", {
      name: "usageSync.deviceFilter.title: usageSync.thisDevice",
    });
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ device: undefined }),
    );

    await user.click(trigger);
    await user.click(
      await screen.findByRole("menuitem", { name: /usageSync.allDevices/ }),
    );

    await waitFor(() =>
      expect(usageHeroMock).toHaveBeenLastCalledWith(
        expect.objectContaining({ device: "all" }),
      ),
    );
    expect(useProviderStatsMock).toHaveBeenLastCalledWith(
      expect.anything(),
      expect.objectContaining({ device: "all" }),
      expect.anything(),
    );
    expect(requestLogTableMock).toHaveBeenLastCalledWith(
      expect.objectContaining({
        device: "all",
        deviceNames: expect.objectContaining({ "11111111-aaaa": "Windows PC" }),
      }),
    );
    // NAS shares daily totals only and is three days stale: both are explained.
    expect(
      screen.getByText("usageSync.notice.dailyOnly:NAS"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("usageSync.notice.outdated:NAS"),
    ).toBeInTheDocument();
    // NAS holds 4 requests of id-less history: the All view says so.
    expect(screen.getByText("usageSync.accuracy.exact")).toBeInTheDocument();
    expect(
      screen.getByText(/^usageSync\.accuracy\.unverified:4,NAS 4/),
    ).toBeInTheDocument();
    expect(
      screen.getByText("usageSync.accuracy.unknownOverlap"),
    ).toBeInTheDocument();
    expect(screen.getByText("usageSync.accuracy.estimate")).toBeInTheDocument();
  });

  it("filters to a single remote device", async () => {
    const user = userEvent.setup();
    renderDashboard();
    await user.click(
      screen.getByRole("button", {
        name: "usageSync.deviceFilter.title: usageSync.thisDevice",
      }),
    );
    await user.click(
      await screen.findByRole("menuitem", { name: /Windows PC/ }),
    );
    await waitFor(() =>
      expect(usageHeroMock).toHaveBeenLastCalledWith(
        expect.objectContaining({ device: "11111111-aaaa" }),
      ),
    );
    expect(
      screen.queryByText(/usageSync.notice.outdated/),
    ).not.toBeInTheDocument();
  });

  it("falls back to this device when the selected device disappears", async () => {
    const user = userEvent.setup();
    const { rerender } = renderDashboard();
    await user.click(
      screen.getByRole("button", {
        name: "usageSync.deviceFilter.title: usageSync.thisDevice",
      }),
    );
    await user.click(
      await screen.findByRole("menuitem", { name: /Windows PC/ }),
    );
    overviewMock.mockReturnValue({
      data: overview({ devices: overview().devices.slice(1) }),
    });
    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <UsageDashboard />
      </QueryClientProvider>,
    );
    await waitFor(() =>
      expect(usageHeroMock).toHaveBeenLastCalledWith(
        expect.objectContaining({ device: undefined }),
      ),
    );
  });
});
