import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TaskUsageTable } from "@/components/usage/TaskUsageTable";
import type { AgentTaskUsageQueryFilter } from "@/lib/query/usage";
import type { AgentTaskUsageRow, AgentUsageMeasure } from "@/types/usage";
import {
  createAgentTaskUsageRow as row,
  createAgentUsageCapability,
  createAgentUsageMeasure as measure,
  createAgentUsageSourceDimension as sourceDimension,
} from "../fixtures/agentUsage";

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

const taskUsageTable = (
  props: Partial<ComponentProps<typeof TaskUsageTable>> = {},
) => (
  <TaskUsageTable
    range={{ preset: "today" }}
    refreshIntervalMs={0}
    {...props}
  />
);

const renderTable = (
  props: Partial<ComponentProps<typeof TaskUsageTable>> = {},
) => render(taskUsageTable(props));

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
        createAgentUsageCapability("claude", { supportsDescendants: true }),
        createAgentUsageCapability("codex", {
          supportsDescendants: true,
          requestCountSemantics: "agent_call",
          tokenStatus: "partial",
          costStatus: "partial",
        }),
        createAgentUsageCapability("hermes", {
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
    renderTable({
      range: { preset: "custom", customStartDate: 100, customEndDate: 200 },
    });

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

  it("clamps the current page when refreshed totals shrink", async () => {
    let shrunk = false;
    useAgentTaskUsageMock.mockImplementation(
      (currentFilter: AgentTaskUsageQueryFilter) => {
        const total = shrunk ? 21 : 41;
        return {
          data: {
            items: [row()],
            total,
            limit: 20,
            offset: currentFilter.offset ?? 0,
            hasMore: (currentFilter.offset ?? 0) + 20 < total,
            unattributedUsage: null,
          },
          isLoading: false,
          isError: false,
          isFetching: false,
        };
      },
    );

    const view = renderTable();
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    await waitFor(() => expect(lastFilter()).toMatchObject({ offset: 20 }));
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    await waitFor(() => expect(lastFilter()).toMatchObject({ offset: 40 }));

    shrunk = true;
    view.rerender(taskUsageTable());

    await waitFor(() => {
      expect(lastFilter()).toMatchObject({ offset: 20 });
      expect(screen.getByRole("button", { name: "Next page" })).toBeDisabled();
    });
  });

  it("hides the ordinary empty state while Codex replay has no published snapshot", () => {
    installQueryResult([], 0, null, "rebuilding");
    renderTable({ initialAppType: "codex" });

    expect(screen.getByTestId("codex-replay-status")).toBeInTheDocument();
    expect(
      screen.getByText(/Codex session statistics are being rebuilt/),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("No root tasks match these filters."),
    ).not.toBeInTheDocument();
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

    renderTable();

    expect(
      screen.getByText("Task title not provided · missing-"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("missing-title-session", { exact: true }),
    ).not.toBeInTheDocument();
    expect(useAgentTaskUsageFilterOptionsMock).toHaveBeenCalled();
  });

  it("clears selected title and project when the agent or date scope changes", async () => {
    const view = renderTable({
      range: { preset: "custom", customStartDate: 100, customEndDate: 200 },
    });

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
      taskUsageTable({
        range: { preset: "custom", customStartDate: 101, customEndDate: 201 },
      }),
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
    useAgentTaskUsageMock.mockImplementation(
      (currentFilter: AgentTaskUsageQueryFilter) => ({
        data: {
          items: [],
          total: 0,
          limit: 20,
          offset: currentFilter.offset ?? 0,
          hasMore: false,
          unattributedUsage: currentFilter.titleExact
            ? null
            : unattributedUsage,
        },
        isLoading: false,
        isError: false,
        isFetching: false,
      }),
    );

    renderTable();

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
    await waitFor(() =>
      expect(lastFilter()).toMatchObject({ titleExact: "Build title" }),
    );
    expect(
      screen.queryByTestId("unattributed-usage-summary"),
    ).not.toBeInTheDocument();
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

    renderTable();

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

  it("scopes cost quality to each expanded task measure", () => {
    installQueryResult([
      row({
        descendantSessionCount: 1,
        selfUsage: measure({ totalCostUsd: "0.01" }),
        descendantUsage: measure({ totalCostUsd: "0.02" }),
        descendantUsageStatus: "available",
        totalUsage: measure({ totalCostUsd: "0.03" }),
        sourceDimensions: [
          sourceDimension("claude", "claude_session", {
            costStatus: "estimated",
            isDescendant: false,
          }),
          sourceDimension("claude", "claude_session", {
            costStatus: "reported",
            isDescendant: true,
          }),
        ],
      }),
    ]);
    renderTable();
    fireEvent.click(
      screen.getByRole("button", { name: /Self \/ descendants/ }),
    );

    const selfCard = screen
      .getByText("Self")
      .closest<HTMLDivElement>(".rounded-md");
    const descendantCard = screen
      .getByText("Descendants")
      .closest<HTMLDivElement>(".rounded-md");
    expect(selfCard).not.toBeNull();
    expect(descendantCard).not.toBeNull();
    expect(within(selfCard!).getByText(/Cost:/)).toHaveTextContent(
      "Cost: ≈$0.0100",
    );
    expect(within(descendantCard!).getByText(/Cost:/)).toHaveTextContent(
      "Cost: $0.0200",
    );
  });

  it("keeps partial, sync-window and unavailable semantics truthful", async () => {
    const partial = row({
      sessionId: "partial",
      rootSessionId: "partial",
      selfUsage: measure({
        inputTokens: 3,
        outputTokens: 4,
        cacheCreationTokens: null,
        totalCostUsd: null,
        requestCountSemantics: "agent_call",
        partial: true,
      }),
      partial: true,
    });
    const hermes = row({
      appType: "hermes",
      sessionId: "hermes",
      rootSessionId: "hermes",
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
      partial: true,
    });
    const unavailable = row({
      sessionId: "unavailable",
      rootSessionId: "unavailable",
      selfUsage: null,
      precision: "unavailable",
      partial: true,
    });
    installQueryResult([partial, hermes, unavailable]);

    renderTable();

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
    const view = renderTable();
    expect(screen.getByRole("status", { name: "Loading" })).toBeInTheDocument();

    useAgentTaskUsageMock.mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: true,
      error: new Error("fixture failure"),
      isFetching: false,
    });
    view.rerender(taskUsageTable());
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
    view.rerender(taskUsageTable());
    expect(
      screen.getByText("No root tasks match these filters."),
    ).toBeInTheDocument();
  });
});
