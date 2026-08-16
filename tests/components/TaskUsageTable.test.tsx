import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TaskUsageTable } from "@/components/usage/TaskUsageTable";
import type { AgentTaskUsageQueryFilter } from "@/lib/query/usage";
import type {
  AgentTaskUsageRow,
  AgentUsageCapability,
  AgentUsageMeasure,
  AgentUsageSourceDimension,
} from "@/types/usage";

const useAgentTaskUsageMock = vi.hoisted(() => vi.fn());
const useAgentTaskUsageFilterOptionsMock = vi.hoisted(() => vi.fn());
const useAgentUsageCapabilitiesMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key,
  }),
}));

vi.mock("@/lib/query/usage", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/query/usage")>(
      "@/lib/query/usage",
    );
  return {
    ...actual,
    useAgentTaskUsage: (...args: unknown[]) => useAgentTaskUsageMock(...args),
    useAgentTaskUsageFilterOptions: (...args: unknown[]) =>
      useAgentTaskUsageFilterOptionsMock(...args),
    useAgentUsageCapabilities: (...args: unknown[]) =>
      useAgentUsageCapabilitiesMock(...args),
  };
});

const measure = (
  overrides: Partial<AgentUsageMeasure> = {},
): AgentUsageMeasure => ({
  dataSource: "fixture",
  requestCount: 2,
  inputTokens: 3,
  outputTokens: 4,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
  totalCostUsd: "0.001",
  precision: "request_exact",
  timeSemantics: "event_time",
  requestCountSemantics: "assistant_message",
  partial: false,
  warnings: [],
  ...overrides,
});

const sourceDimension = (
  overrides: Partial<AgentUsageSourceDimension> = {},
): AgentUsageSourceDimension => ({
  providerId: "_codex_session",
  model: "fixture-codex-model",
  requestModel: "fixture-codex-model",
  pricingModel: "fixture-codex-model",
  dataSource: "codex_session",
  inputTokenSemantics: 0,
  sourceIdentity: "",
  profileId: "",
  databaseIdentity: "",
  baseUrlDigest: "",
  billingMode: "",
  task: "",
  sourceVersion: "",
  syncWindowStart: 0,
  syncWindowEnd: 0,
  apiCallCount: null,
  cacheWriteTokens: null,
  reasoningTokens: null,
  costStatus: "estimated",
  costSource: "model_pricing",
  costDeltaKind: null,
  correctionState: null,
  rangePartial: false,
  ...overrides,
});

const row = (
  overrides: Partial<AgentTaskUsageRow> = {},
): AgentTaskUsageRow => ({
  appType: "claude",
  sessionId: "root-session",
  rootSessionId: "root-session",
  root: {
    appType: "claude",
    sessionId: "root-session",
    parentSessionId: null,
    rootSessionId: "root-session",
    nodeKind: "root",
    relationConfidence: "explicit",
    title: "Root task",
    projectDir: "/workspace/project",
    sourcePath: null,
    createdAt: 100,
    lastActiveAt: 200,
    lastSyncedAt: 200,
  },
  selfUsage: measure(),
  descendantUsage: null,
  descendantUsageStatus: "not_applicable",
  totalUsage: measure(),
  descendantSessionCount: 0,
  precision: "request_exact",
  partial: false,
  warnings: [],
  sourceDimensions: [],
  ...overrides,
});

const capability = (
  appType: AgentUsageCapability["appType"],
  overrides: Partial<AgentUsageCapability> = {},
): AgentUsageCapability => ({
  appType,
  sessionEnumeration: "supported",
  usageStatus: "supported",
  supportsDescendants: appType === "claude" || appType === "codex",
  tokenStatus: "supported",
  costStatus: "supported",
  precision: "request_exact",
  timeSemantics: "event_time",
  requestCountSemantics: "assistant_message",
  notes: "fixture",
  ...overrides,
});

const installQueryResult = (
  items: AgentTaskUsageRow[],
  total = items.length,
  unattributedUsage: AgentUsageMeasure | null = null,
  dataStatus: "ready" | "rebuilding_with_snapshot" | "rebuilding" = "ready",
) => {
  useAgentTaskUsageMock.mockReturnValue({
    data: {
      items,
      total,
      limit: 20,
      offset: 0,
      hasMore: total > items.length,
      unattributedUsage,
      dataStatus,
    },
    isLoading: false,
    isError: false,
    isFetching: false,
  });
};

const lastFilter = (): AgentTaskUsageQueryFilter =>
  useAgentTaskUsageMock.mock.calls.at(-1)?.[0] as AgentTaskUsageQueryFilter;

const setContainerWidth = (width: number) => {
  const descriptor = Object.getOwnPropertyDescriptor(
    HTMLElement.prototype,
    "clientWidth",
  );
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    get: () => width,
  });
  return () => {
    if (descriptor) {
      Object.defineProperty(HTMLElement.prototype, "clientWidth", descriptor);
    } else {
      delete (HTMLElement.prototype as { clientWidth?: number }).clientWidth;
    }
  };
};

describe("TaskUsageTable", () => {
  beforeEach(() => {
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
    useAgentTaskUsageMock.mockReset();
    useAgentTaskUsageFilterOptionsMock.mockReset();
    useAgentUsageCapabilitiesMock.mockReset();
    useAgentTaskUsageFilterOptionsMock.mockReturnValue({
      data: {
        titles: ["Build title", "Another title"],
        projects: [
          { projectDir: "/workspace/cc-switch" },
          { projectDir: "/workspace/other" },
        ],
      },
      isLoading: false,
      isError: false,
      isFetching: false,
    });
    useAgentUsageCapabilitiesMock.mockReturnValue({
      data: [
        capability("claude"),
        capability("codex", {
          requestCountSemantics: "agent_call",
          tokenStatus: "partial",
          costStatus: "partial",
        }),
        capability("hermes", {
          usageStatus: "partial",
          tokenStatus: "partial",
          costStatus: "partial",
          precision: "sync_window_delta",
          timeSemantics: "sync_window_end",
          requestCountSemantics: "unavailable",
        }),
      ],
      isLoading: false,
      isError: false,
    });
    installQueryResult([row()], 21);
  });

  it("passes app/date and exact combobox selections with limit/offset", async () => {
    render(
      <TaskUsageTable
        range={{ preset: "custom", customStartDate: 100, customEndDate: 200 }}
        refreshIntervalMs={0}
      />,
    );

    expect(lastFilter()).toMatchObject({
      rangeSelection: {
        preset: "custom",
        customStartDate: 100,
        customEndDate: 200,
      },
      limit: 20,
      offset: 0,
    });

    fireEvent.change(screen.getByLabelText("Agent"), {
      target: { value: "codex" },
    });
    fireEvent.click(screen.getByRole("combobox", { name: "Task title" }));
    fireEvent.click(screen.getByText("Build title"));
    fireEvent.click(screen.getByRole("combobox", { name: "Project" }));
    fireEvent.click(screen.getByText("cc-switch", { exact: true }));

    await waitFor(() =>
      expect(lastFilter()).toMatchObject({
        appType: "codex",
        titleExact: "Build title",
        projectDirExact: "/workspace/cc-switch",
        rangeSelection: {
          preset: "custom",
          customStartDate: 100,
          customEndDate: 200,
        },
        offset: 0,
      }),
    );
    expect(lastFilter().title).toBeUndefined();
    expect(lastFilter().project).toBeUndefined();
    expect(lastFilter().projectDir).toBeUndefined();
    expect(screen.queryByText("Project directory")).not.toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Codex" })).toBeInTheDocument();
    expect(
      screen.queryByRole("option", { name: /Codex.*Partial/ }),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    await waitFor(() =>
      expect(lastFilter()).toMatchObject({ limit: 20, offset: 20 }),
    );
    expect(screen.queryByLabelText("Rows per page")).not.toBeInTheDocument();
  });

  it("hides the ordinary empty state while Codex replay has no published snapshot", () => {
    installQueryResult([], 0, null, "rebuilding");
    render(
      <TaskUsageTable
        range={{ preset: "today" }}
        refreshIntervalMs={0}
        initialAppType="codex"
      />,
    );

    expect(screen.getByTestId("codex-replay-status")).toBeInTheDocument();
    expect(screen.getByText(/Codex session statistics are being rebuilt/)).toBeInTheDocument();
    expect(screen.queryByText("No root tasks match these filters.")).not.toBeInTheDocument();
    expect(screen.queryByText("Unattributed sessions")).not.toBeInTheDocument();
  });

  it("does not turn a missing native title into a UUID or a candidate", () => {
    installQueryResult([
      row({
        sessionId: "missing-title-session",
        rootSessionId: "missing-title-session",
        root: {
          ...row().root!,
          sessionId: "missing-title-session",
          rootSessionId: "missing-title-session",
          title: null,
        },
      }),
    ]);

    render(
      <TaskUsageTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );

    expect(
      screen.getByText("Task title not provided · missing-"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("missing-title-session", { exact: true }),
    ).not.toBeInTheDocument();
    expect(useAgentTaskUsageFilterOptionsMock).toHaveBeenCalled();
  });

  it("clears selected title and project when the agent or date scope changes", async () => {
    const view = render(
      <TaskUsageTable
        range={{ preset: "custom", customStartDate: 100, customEndDate: 200 }}
        refreshIntervalMs={0}
      />,
    );

    fireEvent.click(screen.getByRole("combobox", { name: "Task title" }));
    fireEvent.click(screen.getByText("Build title"));
    fireEvent.click(screen.getByRole("combobox", { name: "Project" }));
    fireEvent.click(screen.getByText("cc-switch", { exact: true }));

    fireEvent.change(screen.getByLabelText("Agent"), {
      target: { value: "codex" },
    });
    await waitFor(() =>
      expect(lastFilter()).toMatchObject({
        appType: "codex",
        titleExact: undefined,
        projectDirExact: undefined,
      }),
    );
    expect(
      screen.getByRole("combobox", { name: "Task title" }),
    ).toHaveTextContent("Select task title");
    expect(screen.getByRole("combobox", { name: "Project" })).toHaveTextContent(
      "Select project",
    );

    view.rerender(
      <TaskUsageTable
        range={{ preset: "custom", customStartDate: 101, customEndDate: 201 }}
        refreshIntervalMs={0}
      />,
    );
    await waitFor(() =>
      expect(lastFilter()).toMatchObject({
        rangeSelection: {
          preset: "custom",
          customStartDate: 101,
          customEndDate: 201,
        },
        titleExact: undefined,
        projectDirExact: undefined,
      }),
    );
  });

  it("renders a complete narrow card without horizontal table clipping", async () => {
    const longTitle =
      "A very long Codex task title that must remain inspectable";
    const longProject =
      "/workspace/cc-switch/projects/a/very/long/project/path";
    installQueryResult([
      row({
        root: {
          ...row().root!,
          title: longTitle,
          projectDir: longProject,
        },
      }),
    ]);

    render(
      <TaskUsageTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );

    await waitFor(() =>
      expect(screen.getByTestId("task-usage-cards")).toBeInTheDocument(),
    );
    expect(screen.getByTitle(longTitle)).toBeInTheDocument();
    expect(
      screen.getByText("Claude Code - path", { exact: true }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(longProject, { exact: true }),
    ).not.toBeInTheDocument();
    expect(screen.queryByTitle(longProject)).not.toBeInTheDocument();
    expect(screen.getAllByTestId(/^task-row-/)).toHaveLength(1);
  });

  it("uses the compressed three-column table only when the container is wide enough", async () => {
    const restoreWidth = setContainerWidth(1400);
    const view = render(
      <TaskUsageTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );

    await waitFor(() =>
      expect(screen.getByTestId("task-usage-table")).toBeInTheDocument(),
    );
    expect(screen.queryByTestId("task-usage-cards")).not.toBeInTheDocument();
    const table = within(screen.getByTestId("task-usage-table"));
    expect(table.getByText("Task")).toBeInTheDocument();
    expect(table.queryByText("Project")).not.toBeInTheDocument();
    expect(table.getByText("Derived total")).toBeInTheDocument();
    expect(table.getByText("Count")).toBeInTheDocument();
    expect(table.getAllByRole("columnheader")).toHaveLength(3);
    expect(screen.queryByText("Data status")).not.toBeInTheDocument();
    expect(screen.queryByText("Request-exact")).not.toBeInTheDocument();
    view.unmount();
    restoreWidth();
  });

  it("marks estimated task costs with an approximation sign and tooltip", async () => {
    installQueryResult([
      row({
        appType: "codex",
        sourceDimensions: [sourceDimension()],
        totalUsage: measure({ totalCostUsd: "0.0023" }),
      }),
    ]);

    render(
      <TaskUsageTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );

    expect(screen.getByText("≈$0.0023")).toBeInTheDocument();
    const costTrigger = screen.getAllByLabelText(
      /API-equivalent estimate from current model pricing/,
    )[0];
    fireEvent.focus(costTrigger);
    await waitFor(() =>
      expect(
        screen.getAllByText(
          "API-equivalent estimate from current model pricing; not a Codex subscription bill.",
        ).length,
      ).toBeGreaterThan(0),
    );
  });

  it("shows unattributed Codex usage outside task rows and hides it after metadata filters", async () => {
    const unattributedUsage = measure({
      dataSource: "proxy",
      requestCount: 3,
      inputTokens: 10,
      outputTokens: 5,
      cacheReadTokens: 20,
      totalCostUsd: "1.25",
      requestCountSemantics: "http_request",
    });
    useAgentTaskUsageMock.mockImplementation((currentFilter: AgentTaskUsageQueryFilter) => ({
      data: {
        items: [],
        total: 0,
        limit: 20,
        offset: currentFilter.offset ?? 0,
        hasMore: false,
        unattributedUsage: currentFilter.titleExact ? null : unattributedUsage,
      },
      isLoading: false,
      isError: false,
      isFetching: false,
    }));

    render(
      <TaskUsageTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );

    const summary = screen.getByTestId("unattributed-usage-summary");
    expect(summary).toHaveTextContent("Unattributed sessions");
    expect(summary).toHaveTextContent("3 HTTP requests");
    expect(summary).toHaveTextContent("35 tokens");
    expect(summary).toHaveTextContent("$1.2500");
    expect(summary).toHaveAttribute(
      "aria-label",
      "These Codex proxy requests are included in the top cost total but have no verifiable native session event, so they are not assigned to a specific task.",
    );

    fireEvent.click(screen.getByRole("combobox", { name: "Task title" }));
    fireEvent.click(screen.getByText("Build title"));
    await waitFor(() => expect(lastFilter()).toMatchObject({ titleExact: "Build title" }));
    expect(screen.queryByTestId("unattributed-usage-summary")).not.toBeInTheDocument();
  });

  it("keeps one root row for a 100-child aggregate and expands only the compact breakdown", () => {
    const root = row({
      descendantSessionCount: 100,
      descendantUsage: measure({
        inputTokens: 100,
        outputTokens: 200,
        requestCount: 100,
      }),
      totalUsage: measure({
        inputTokens: 103,
        outputTokens: 204,
        requestCount: 102,
      }),
    });
    installQueryResult([root]);

    render(
      <TaskUsageTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );

    expect(screen.getByTestId("task-usage-cards")).toBeInTheDocument();
    expect(screen.getAllByTestId(/^task-row-/)).toHaveLength(1);
    expect(screen.queryByTestId(/^task-breakdown-/)).not.toBeInTheDocument();
    expect(screen.queryByText(/child session/i)).not.toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: /Self \/ descendants/ }),
    );

    expect(screen.getByTestId(/^task-breakdown-/)).toBeInTheDocument();
    expect(screen.getByText("Self")).toBeInTheDocument();
    expect(screen.getByText("Descendants")).toBeInTheDocument();
    expect(screen.getAllByTestId(/^task-row-/)).toHaveLength(1);
    expect(screen.queryByText("Request-exact")).not.toBeInTheDocument();
  });

  it("explains descendants with no activity in the selected range", () => {
    installQueryResult([
      row({
        descendantSessionCount: 147,
        descendantUsageStatus: "no_activity_in_range",
        descendantUsage: null,
      }),
    ]);

    render(
      <TaskUsageTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: /Self \/ descendants/ }),
    );
    expect(
      screen.getByText("No descendant activity in the selected time range"),
    ).toBeInTheDocument();
    expect(screen.queryByText("Usage unavailable")).not.toBeInTheDocument();
  });

  it("keeps partial, sync-window and unavailable semantics truthful", async () => {
    const partial = row({
      sessionId: "partial",
      rootSessionId: "partial",
      root: {
        ...row().root!,
        sessionId: "partial",
        rootSessionId: "partial",
        title: "Partial task",
      },
      selfUsage: measure({
        inputTokens: 3,
        outputTokens: 4,
        cacheCreationTokens: null,
        totalCostUsd: null,
        requestCountSemantics: "agent_call",
        partial: true,
      }),
      totalUsage: measure({
        inputTokens: 3,
        outputTokens: 4,
        cacheCreationTokens: null,
        totalCostUsd: null,
        requestCountSemantics: "agent_call",
        partial: true,
      }),
      precision: "request_exact",
      partial: true,
    });
    const hermes = row({
      appType: "hermes",
      sessionId: "hermes",
      rootSessionId: "hermes",
      root: {
        ...row().root!,
        appType: "hermes",
        sessionId: "hermes",
        rootSessionId: "hermes",
        title: "Hermes sync window",
      },
      selfUsage: measure({
        inputTokens: 5,
        outputTokens: null,
        cacheReadTokens: null,
        cacheCreationTokens: null,
        requestCount: null,
        totalCostUsd: null,
        precision: "sync_window_delta",
        timeSemantics: "sync_window_end",
        requestCountSemantics: "unavailable",
        partial: true,
      }),
      totalUsage: measure({
        inputTokens: 5,
        outputTokens: null,
        cacheReadTokens: null,
        cacheCreationTokens: null,
        requestCount: null,
        totalCostUsd: null,
        precision: "sync_window_delta",
        timeSemantics: "sync_window_end",
        requestCountSemantics: "unavailable",
        partial: true,
      }),
      precision: "sync_window_delta",
      partial: true,
    });
    const unavailable = row({
      sessionId: "unavailable",
      rootSessionId: "unavailable",
      root: {
        ...row().root!,
        sessionId: "unavailable",
        rootSessionId: "unavailable",
        title: "Unavailable task",
      },
      selfUsage: null,
      totalUsage: null,
      precision: "unavailable",
      partial: true,
    });
    installQueryResult([partial, hermes, unavailable]);

    render(
      <TaskUsageTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );

    expect(screen.getByText(/7\+/)).toBeInTheDocument();
    expect(screen.getByText("Agent calls")).toBeInTheDocument();
    expect(screen.queryByText(/Sync-window delta/)).not.toBeInTheDocument();
    expect(screen.queryByText("Partial")).not.toBeInTheDocument();
    expect(screen.queryByText("Request-exact")).not.toBeInTheDocument();
    const partialWarning = screen.getAllByRole("img", {
      name: /Some usage fields are partial or unavailable\./,
    });
    expect(partialWarning).toHaveLength(3);
    expect(
      screen.queryByText("Some usage fields are partial or unavailable."),
    ).not.toBeInTheDocument();
    fireEvent.focus(partialWarning[0]);
    await waitFor(() =>
      expect(
        screen.getAllByText("Some usage fields are partial or unavailable.")
          .length,
      ).toBeGreaterThan(0),
    );
    const syncWarning = screen.getByRole("img", {
      name: /Sync-window increment; not per-request\./,
    });
    fireEvent.focus(syncWarning);
    await waitFor(() =>
      expect(
        screen.getAllByText("Sync-window increment; not per-request.").length,
      ).toBeGreaterThan(0),
    );
    expect(screen.getAllByText(/Count unavailable/).length).toBeGreaterThan(0);
    expect(screen.getAllByText("Unavailable").length).toBeGreaterThan(0);
    expect(screen.queryByText("HTTP requests")).not.toBeInTheDocument();
  });

  it("renders loading, error and empty states instead of fabricating metrics", () => {
    useAgentTaskUsageMock.mockReturnValue({
      data: undefined,
      isLoading: true,
      isError: false,
      isFetching: true,
    });
    const view = render(
      <TaskUsageTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );
    expect(screen.getByRole("status", { name: "Loading" })).toBeInTheDocument();

    useAgentTaskUsageMock.mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: true,
      error: new Error("fixture failure"),
      isFetching: false,
    });
    view.rerender(
      <TaskUsageTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Unable to load tasks");

    useAgentTaskUsageMock.mockReturnValue({
      data: {
        items: [],
        total: 0,
        limit: 20,
        offset: 0,
        hasMore: false,
        unattributedUsage: null,
      },
      isLoading: false,
      isError: false,
      isFetching: false,
    });
    view.rerender(
      <TaskUsageTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );
    expect(
      screen.getByText("No root tasks match these filters."),
    ).toBeInTheDocument();
  });
});
