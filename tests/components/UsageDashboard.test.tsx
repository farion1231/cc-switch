import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageDashboard } from "@/components/usage/UsageDashboard";

const useProviderStatsMock = vi.hoisted(() => vi.fn());
const useModelStatsMock = vi.hoisted(() => vi.fn());
const usageHeroMock = vi.hoisted(() => vi.fn());
const rebuildAgentSessionUsageMock = vi.hoisted(() => vi.fn());
const toastSuccessMock = vi.hoisted(() => vi.fn());
const toastWarningMock = vi.hoisted(() => vi.fn());
const toastErrorMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (
      key: string,
      options?: string | { defaultValue?: string; [key: string]: unknown },
    ) => {
      if (typeof options === "string") return options;
      if (key.includes("usage.rebuildAgentSessionUsage.providers.")) {
        const provider = key.split(".").at(-1);
        return {
          claude: "Claude",
          codex: "Codex",
          grokbuild: "Grok Build",
          opencode: "OpenCode",
          hermes: "Hermes",
          pi: "Pi",
        }[provider ?? ""] ?? provider ?? key;
      }
      if (key === "usage.rebuildAgentSessionUsage.none") return "none";
      if (options?.defaultValue) return options.defaultValue;
      if (key === "usage.rebuildAgentSessionUsage.confirmMessage") {
        return `Selected providers: ${String(options?.providers)}. A safety backup will be created; failures keep the previous published generation.`;
      }
      if (key === "usage.rebuildAgentSessionUsage.completed") {
        return `Published: ${String(options?.published)}. Kept previous: ${String(options?.keptPrevious)}.`;
      }
      if (key === "usage.rebuildAgentSessionUsage.syncDetails") {
        return `Sync details: ${String(options?.errors)} errors, ${String(options?.deferred)} deferred files.`;
      }
      return key;
    },
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

vi.mock("@/lib/api/usage", () => ({
  usageApi: {
    rebuildAgentSessionUsage: rebuildAgentSessionUsageMock,
  },
}));

vi.mock("sonner", () => ({
  toast: {
    success: toastSuccessMock,
    warning: toastWarningMock,
    error: toastErrorMock,
  },
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

vi.mock("@/components/usage/RequestLogTable", () => ({
  RequestLogTable: () => <div data-testid="request-log-table" />,
}));

vi.mock("@/components/usage/ProviderStatsTable", () => ({
  ProviderStatsTable: () => <div data-testid="provider-stats-table" />,
}));

vi.mock("@/components/usage/ModelStatsTable", () => ({
  ModelStatsTable: () => <div data-testid="model-stats-table" />,
}));

vi.mock("@/components/usage/TaskUsageTable", () => ({
  TaskUsageTable: () => <div data-testid="task-usage-table" />,
}));

vi.mock("@/components/usage/PricingConfigPanel", () => ({
  PricingConfigPanel: () => <div data-testid="pricing-config-panel" />,
}));

vi.mock("@/components/usage/UsageDateRangePicker", () => ({
  UsageDateRangePicker: () => <button type="button">date-range</button>,
}));

vi.mock("@/components/ui/select", () => ({
  Select: ({ value, onValueChange, children }: any) => (
    <div data-testid={`select-${value}`}>
      {children}
      <button type="button" onClick={() => onValueChange?.("5000")}>
        choose-5000
      </button>
    </div>
  ),
  SelectTrigger: ({ children, ...props }: any) => (
    <button type="button" {...props}>
      {children}
    </button>
  ),
  SelectValue: () => null,
  SelectContent: ({ children }: any) => <div>{children}</div>,
  SelectItem: ({ children, ...props }: any) => <div {...props}>{children}</div>,
}));

const createQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });

const renderDashboard = (
  props: ComponentProps<typeof UsageDashboard> = {},
  queryClient = createQueryClient(),
) => {
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
    usageHeroMock.mockReset();
    rebuildAgentSessionUsageMock.mockReset();
    toastSuccessMock.mockReset();
    toastWarningMock.mockReset();
    toastErrorMock.mockReset();
    useProviderStatsMock.mockReturnValue({ data: [] });
    useModelStatsMock.mockReturnValue({ data: [] });
  });

  it("uses the saved refresh interval when mounted", () => {
    renderDashboard({ refreshIntervalMs: 5000 });

    expect(screen.getByTestId("select-5000")).toBeInTheDocument();
  });

  it("filters usage queries to Pi", async () => {
    renderDashboard();

    fireEvent.click(screen.getByRole("button", { name: "usage.appFilter.pi" }));

    await waitFor(() =>
      expect(useProviderStatsMock).toHaveBeenLastCalledWith(
        expect.anything(),
        { appType: "pi" },
        expect.anything(),
      ),
    );
    expect(useModelStatsMock).toHaveBeenLastCalledWith(
      expect.anything(),
      { appType: "pi", providerName: undefined },
      expect.anything(),
    );
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ appType: "pi" }),
    );
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

  it("keeps the existing controls while exposing the task view", async () => {
    renderDashboard();

    expect(screen.getByTestId("usage-hero")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "date-range" }),
    ).toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("tab", { name: "Task Statistics" }),
    );

    await waitFor(() =>
      expect(screen.getByTestId("task-usage-table")).toBeInTheDocument(),
    );
    expect(screen.getByTestId("usage-hero")).toBeInTheDocument();
  });

  it("defaults to all supported providers for the all filter and one provider for a supported filter", async () => {
    const { unmount } = renderDashboard();
    fireEvent.click(
      screen.getByRole("button", {
        name: /usage\.rebuildAgentSessionUsage\.title/,
      }),
    );

    expect(screen.getAllByRole("checkbox")).toHaveLength(6);
    expect(screen.getAllByRole("checkbox").every((checkbox) =>
      (checkbox as HTMLInputElement).checked,
    )).toBe(true);

    unmount();
    renderDashboard();
    fireEvent.click(screen.getByRole("button", { name: "usage.appFilter.codex" }));
    fireEvent.click(
      screen.getByRole("button", {
        name: /usage\.rebuildAgentSessionUsage\.title/,
      }),
    );

    const checkboxes = screen.getAllByRole("checkbox") as HTMLInputElement[];
    expect(checkboxes.filter((checkbox) => checkbox.checked)).toHaveLength(1);
    expect(screen.getByRole("checkbox", { name: "Codex" })).toBeChecked();
  });

  it("lets the user adjust selection and confirms named providers with backup retention copy", async () => {
    rebuildAgentSessionUsageMock.mockResolvedValue({ providers: [] });
    renderDashboard();
    fireEvent.click(
      screen.getByRole("button", {
        name: /usage\.rebuildAgentSessionUsage\.title/,
      }),
    );

    fireEvent.click(screen.getByRole("checkbox", { name: "Claude" }));
    fireEvent.click(
      screen.getByRole("button", {
        name: "usage.rebuildAgentSessionUsage.action",
      }),
    );
    expect(
      screen.getByText(
        "Selected providers: Codex, Grok Build, OpenCode, Hermes, Pi. A safety backup will be created; failures keep the previous published generation.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "usage.rebuildAgentSessionUsage.confirmAction",
      }),
    ).toBeInTheDocument();
  });

  it("calls the generic endpoint with the nested request and invalidates all usage queries", async () => {
    rebuildAgentSessionUsageMock.mockResolvedValue({
      providers: [
        {
          appType: "codex",
          status: "published",
          syncResult: {
            imported: 1,
            skipped: 0,
            filesScanned: 1,
            suspectedDuplicates: 0,
            deferredFiles: 0,
            errors: [],
          },
        },
      ],
    });
    const queryClient = createQueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    renderDashboard({}, queryClient);

    fireEvent.click(screen.getByRole("button", { name: "usage.appFilter.codex" }));
    fireEvent.click(
      screen.getByRole("button", {
        name: /usage\.rebuildAgentSessionUsage\.title/,
      }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "usage.rebuildAgentSessionUsage.action",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "usage.rebuildAgentSessionUsage.confirmAction",
      }),
    );

    await waitFor(() =>
      expect(rebuildAgentSessionUsageMock).toHaveBeenCalledWith({
        appTypes: ["codex"],
      }),
    );
    await waitFor(() =>
      expect(invalidateQueries).toHaveBeenCalledWith({
        queryKey: ["usage"],
      }),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "Published: Codex. Kept previous: none.",
    );
  });

  it("reports mixed published and kept-previous results with a warning", async () => {
    rebuildAgentSessionUsageMock.mockResolvedValue({
      providers: [
        {
          appType: "claude",
          status: "keptPrevious",
          syncResult: {
            imported: 0,
            skipped: 0,
            filesScanned: 1,
            suspectedDuplicates: 0,
            deferredFiles: 1,
            errors: ["source unavailable"],
          },
        },
        {
          appType: "codex",
          status: "published",
          syncResult: {
            imported: 2,
            skipped: 0,
            filesScanned: 2,
            suspectedDuplicates: 0,
            deferredFiles: 0,
            errors: [],
          },
        },
      ],
    });
    renderDashboard();
    fireEvent.click(
      screen.getByRole("button", {
        name: /usage\.rebuildAgentSessionUsage\.title/,
      }),
    );
    fireEvent.click(screen.getByRole("checkbox", { name: "Grok Build" }));
    fireEvent.click(
      screen.getByRole("button", {
        name: "usage.rebuildAgentSessionUsage.action",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "usage.rebuildAgentSessionUsage.confirmAction",
      }),
    );

    await waitFor(() => expect(toastWarningMock).toHaveBeenCalledTimes(1));
    expect(toastWarningMock).toHaveBeenCalledWith(
      "Published: Codex. Kept previous: Claude. Sync details: 1 errors, 1 deferred files.",
    );
    expect(toastSuccessMock).not.toHaveBeenCalled();
  });

  it("disables rebuild controls while the provider command is in flight", async () => {
    let resolveRebuild: (value: unknown) => void = () => {};
    rebuildAgentSessionUsageMock.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveRebuild = resolve;
        }),
    );
    renderDashboard();
    fireEvent.click(
      screen.getByRole("button", {
        name: /usage\.rebuildAgentSessionUsage\.title/,
      }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "usage.rebuildAgentSessionUsage.action",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "usage.rebuildAgentSessionUsage.confirmAction",
      }),
    );

    await waitFor(() => expect(rebuildAgentSessionUsageMock).toHaveBeenCalled());
    expect(
      screen.getByRole("button", {
        name: "usage.rebuildAgentSessionUsage.action",
      }),
    ).toBeDisabled();
    expect(
      (screen.getAllByRole("checkbox")[0] as HTMLInputElement).disabled,
    ).toBe(true);

    resolveRebuild({ providers: [] });
    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledTimes(1),
    );
  });
});
