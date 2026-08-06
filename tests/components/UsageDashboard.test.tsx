import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { Children, type ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageDashboard } from "@/components/usage/UsageDashboard";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";
import { KNOWN_APP_TYPES } from "@/types/usage";

const useProviderStatsMock = vi.hoisted(() => vi.fn());
const useModelStatsMock = vi.hoisted(() => vi.fn());
const useUsageSummaryByAppMock = vi.hoisted(() => vi.fn());
const useHermesUsageMetadataMock = vi.hoisted(() => vi.fn());
const usageHeroPropsMock = vi.hoisted(() => vi.fn());
const usageTrendPropsMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string) => fallback ?? key,
    i18n: {
      resolvedLanguage: "en",
      language: "en",
    },
  }),
}));

vi.mock("framer-motion", () => ({
  motion: {
    div: ({ children, ...props }: any) => <div {...props}>{children}</div>,
  },
}));

vi.mock("@/hooks/useUsageEventBridge", () => ({
  useUsageEventBridge: () => {},
}));

vi.mock("@/lib/query/usage", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/query/usage")>(
      "@/lib/query/usage",
    );
  return {
    ...actual,
    useProviderStats: (...args: unknown[]) => useProviderStatsMock(...args),
    useModelStats: (...args: unknown[]) => useModelStatsMock(...args),
    useUsageSummaryByApp: (...args: unknown[]) =>
      useUsageSummaryByAppMock(...args),
    useHermesUsageMetadata: (...args: unknown[]) =>
      useHermesUsageMetadataMock(...args),
  };
});

vi.mock("@/components/usage/UsageHero", () => ({
  UsageHero: (props: Record<string, unknown>) => {
    usageHeroPropsMock(props);
    return <div data-testid="usage-hero" />;
  },
}));

vi.mock("@/components/usage/UsageTrendChart", () => ({
  UsageTrendChart: (props: Record<string, unknown>) => {
    usageTrendPropsMock(props);
    return <div data-testid="usage-trend" />;
  },
}));

vi.mock("@/components/usage/RequestLogTable", () => ({
  RequestLogTable: () => <div data-testid="request-log-table" />,
}));

vi.mock("@/components/usage/ProviderStatsTable", () => ({
  ProviderStatsTable: () => <div data-testid="provider-stats-table" />,
}));

vi.mock("@/components/usage/ModelStatsTable", () => ({
  ModelStatsTable: () => <div data-testid="model-stats-table" />,
}));

vi.mock("@/components/usage/PricingConfigPanel", () => ({
  PricingConfigPanel: () => <div data-testid="pricing-config-panel" />,
}));

vi.mock("@/components/usage/UsageDateRangePicker", () => ({
  UsageDateRangePicker: () => <button type="button">date-range</button>,
}));

vi.mock("@/components/ui/select", () => ({
  Select: ({ value, onValueChange, children }: any) => {
    const trigger = Children.toArray(children).find(
      (child: any) => child?.props?.["aria-label"],
    ) as any;
    const label = trigger?.props?.["aria-label"] as string | undefined;
    const isHermesFilter =
      label === "usage.hermes.profile" || label === "usage.hermes.task";
    const nextValue =
      label === "usage.hermes.profile"
        ? "v:profile-a"
        : label === "usage.hermes.task"
          ? "v:task-a"
          : "5000";
    return (
      <div
        data-testid={
          isHermesFilter ? `hermes-select-${label}` : `select-${value}`
        }
      >
        {children}
        <button
          type="button"
          data-testid={isHermesFilter ? `choose-${label}` : undefined}
          onClick={() => onValueChange?.(nextValue)}
        >
          {isHermesFilter ? `choose-${label}` : "choose-5000"}
        </button>
      </div>
    );
  },
  SelectTrigger: ({ children, ...props }: any) => (
    <button type="button" {...props}>
      {children}
    </button>
  ),
  SelectValue: () => null,
  SelectContent: ({ children }: any) => <div>{children}</div>,
  SelectItem: ({ children, ...props }: any) => <div {...props}>{children}</div>,
}));

const renderDashboard = (props: ComponentProps<typeof UsageDashboard> = {}) => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <UsageDashboard {...props} />
    </QueryClientProvider>,
  );
};

describe("UsageDashboard", () => {
  beforeEach(() => {
    useProviderStatsMock.mockReset();
    useModelStatsMock.mockReset();
    useUsageSummaryByAppMock.mockReset();
    useHermesUsageMetadataMock.mockReset();
    usageHeroPropsMock.mockReset();
    usageTrendPropsMock.mockReset();
    useProviderStatsMock.mockReturnValue({ data: [] });
    useModelStatsMock.mockReturnValue({ data: [] });
    useUsageSummaryByAppMock.mockReturnValue({ data: [], isLoading: false });
    useHermesUsageMetadataMock.mockReturnValue({
      data: {
        dataSource: "hermes_session",
        precision: "aggregate_delta",
        explanation: "aggregate metadata",
        profiles: ["profile-a"],
        tasks: ["task-a"],
      },
    });
  });

  it("uses the saved refresh interval when mounted", () => {
    renderDashboard({ refreshIntervalMs: 5000 });

    expect(screen.getByTestId("select-5000")).toBeInTheDocument();
  });

  it("exposes Hermes as a usage dashboard app filter", () => {
    expect(KNOWN_APP_TYPES).toContain("hermes");
  });

  it("renders Hermes filters, precision notices, and suppresses request details", () => {
    renderDashboard();

    expect(
      screen.getByRole("button", { name: "usage.appFilter.hermes" }),
    ).toBeInTheDocument();
    expect(screen.getByTestId("request-log-table")).toBeInTheDocument();
    expect(
      screen.queryByTestId("hermes-select-usage.hermes.profile"),
    ).not.toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "usage.appFilter.hermes" }),
    );

    expect(
      screen.getByTestId("hermes-select-usage.hermes.profile"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("hermes-select-usage.hermes.task"),
    ).toBeInTheDocument();
    expect(screen.getByText("profile-a")).toBeInTheDocument();
    expect(screen.getByText("task-a")).toBeInTheDocument();
    expect(screen.getByTestId("hermes-precision-notice")).toBeInTheDocument();
    expect(
      screen.getByTestId("hermes-request-details-notice"),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("request-log-table")).not.toBeInTheDocument();
  });

  it("shows the Hermes precision notice in All mode only when Hermes is in the current summary", () => {
    useUsageSummaryByAppMock.mockReturnValue({
      data: [{ appType: "hermes", summary: {} }],
      isLoading: false,
    });
    renderDashboard();

    expect(screen.getByTestId("hermes-precision-notice")).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "usage.appFilter.claude" }),
    );

    expect(
      screen.queryByTestId("hermes-precision-notice"),
    ).not.toBeInTheDocument();
  });

  it("does not show the Hermes precision notice in All mode without a Hermes bucket", () => {
    useUsageSummaryByAppMock.mockReturnValue({
      data: [{ appType: "claude", summary: {} }],
      isLoading: false,
    });
    renderDashboard();

    expect(
      screen.queryByTestId("hermes-precision-notice"),
    ).not.toBeInTheDocument();
  });

  it("propagates Hermes Profile/task filters and clears them when leaving Hermes", async () => {
    renderDashboard();
    fireEvent.click(
      screen.getByRole("button", { name: "usage.appFilter.hermes" }),
    );
    fireEvent.click(screen.getByTestId("choose-usage.hermes.profile"));
    fireEvent.click(screen.getByTestId("choose-usage.hermes.task"));

    await waitFor(() => {
      const providerCall =
        useProviderStatsMock.mock.calls[
          useProviderStatsMock.mock.calls.length - 1
        ];
      const modelCall =
        useModelStatsMock.mock.calls[useModelStatsMock.mock.calls.length - 1];
      expect(providerCall?.[1]).toMatchObject({
        appType: "hermes",
        profileName: "profile-a",
        task: "task-a",
      });
      expect(modelCall?.[1]).toMatchObject({
        appType: "hermes",
        profileName: "profile-a",
        task: "task-a",
      });
    });

    const heroProps =
      usageHeroPropsMock.mock.calls[
        usageHeroPropsMock.mock.calls.length - 1
      ]?.[0];
    const trendProps =
      usageTrendPropsMock.mock.calls[
        usageTrendPropsMock.mock.calls.length - 1
      ]?.[0];
    expect(heroProps).toMatchObject({
      appType: "hermes",
      profileName: "profile-a",
      task: "task-a",
    });
    expect(trendProps).toMatchObject({
      appType: "hermes",
      profileName: "profile-a",
      task: "task-a",
    });

    fireEvent.click(
      screen.getByRole("button", { name: "usage.appFilter.claude" }),
    );

    await waitFor(() => {
      const providerCall =
        useProviderStatsMock.mock.calls[
          useProviderStatsMock.mock.calls.length - 1
        ];
      expect(providerCall?.[1]).toMatchObject({
        appType: "claude",
        profileName: undefined,
        task: undefined,
      });
    });
    expect(
      screen.queryByTestId("hermes-select-usage.hermes.profile"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("request-log-table")).toBeInTheDocument();
  });

  it("keeps request details for all and non-Hermes app filters", () => {
    renderDashboard();
    expect(screen.getByTestId("request-log-table")).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "usage.appFilter.hermes" }),
    );
    expect(screen.queryByTestId("request-log-table")).not.toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "usage.appFilter.all" }),
    );
    expect(screen.getByTestId("request-log-table")).toBeInTheDocument();
  });

  it("keeps Hermes usage keys in every locale", () => {
    for (const locale of [en, zh, zhTW, ja]) {
      expect(locale.usage.appFilter.hermes).toBeTruthy();
      expect(locale.usage.hermes.profile).toBeTruthy();
      expect(locale.usage.hermes.profilePlaceholder).toBeTruthy();
      expect(locale.usage.hermes.task).toBeTruthy();
      expect(locale.usage.hermes.taskPlaceholder).toBeTruthy();
      expect(locale.usage.hermes.precisionNotice).toBeTruthy();
      expect(locale.usage.hermes.syncWindowNotice).toBeTruthy();
      expect(locale.usage.hermes.requestDetailsUnavailable).toBeTruthy();
      expect(locale.usage.countLabel.requests).toBeTruthy();
      expect(locale.usage.countLabel.hermesApiCalls).toBeTruthy();
      expect(locale.usage.countLabel.mixedActivity).toBeTruthy();
      expect(locale.usage.averageCostLabel.perRequest).toBeTruthy();
      expect(locale.usage.averageCostLabel.perApiCall).toBeTruthy();
      expect(locale.usage.averageCostLabel.perActivity).toBeTruthy();
    }
  });

  it("persists refresh interval changes", async () => {
    const onRefreshIntervalChange = vi.fn().mockResolvedValue(true);
    renderDashboard({ onRefreshIntervalChange });

    fireEvent.click(
      within(screen.getByTestId("select-30000")).getByRole("button", {
        name: "choose-5000",
      }),
    );

    await waitFor(() =>
      expect(onRefreshIntervalChange).toHaveBeenCalledWith(5000),
    );
    expect(screen.getByTestId("select-5000")).toBeInTheDocument();
  });

  it("rolls back optimistic interval changes when persistence fails", async () => {
    const onRefreshIntervalChange = vi.fn().mockResolvedValue(false);
    renderDashboard({ onRefreshIntervalChange });

    fireEvent.click(
      within(screen.getByTestId("select-30000")).getByRole("button", {
        name: "choose-5000",
      }),
    );

    await waitFor(() =>
      expect(onRefreshIntervalChange).toHaveBeenCalledWith(5000),
    );
    await waitFor(() =>
      expect(screen.getByTestId("select-30000")).toBeInTheDocument(),
    );
  });
});
