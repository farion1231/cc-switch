import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, act } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { usageApi } from "@/lib/api/usage";
import {
  normalizeAgentTaskUsageFilter,
  normalizeAgentUsageRange,
  useAgentTaskUsage,
  useAgentTaskUsageFilterOptions,
  usageKeys,
} from "@/lib/query/usage";
import type {
  AgentTaskUsageFilter,
  AgentTaskUsageFilterOptions,
  AgentTaskUsagePage,
  AgentTaskUsageFilterOptionsRequest,
} from "@/types/usage";

const emptyTaskPage: AgentTaskUsagePage = {
  items: [],
  total: 0,
  limit: 20,
  offset: 0,
  hasMore: false,
  unattributedUsage: null,
};

const filterOptions: AgentTaskUsageFilterOptions = {
  titles: [],
  projects: [],
};

function queryWrapper(client: QueryClient) {
  return ({ children }: PropsWithChildren) =>
    createElement(QueryClientProvider, { client }, children);
}

describe("canonical Agent usage data contract", () => {
  it("keeps zero, null/partial, unavailable, and request-count semantics intact", async () => {
    const zero = await usageApi.getAgentSessionUsage({
      appType: "claude",
      sessionId: "claude-usage-zero",
    });
    expect(zero.selfUsage?.inputTokens).toBe(0);
    expect(zero.selfUsage?.cacheCreationTokens).toBe(0);
    expect(zero.selfUsage?.totalCostUsd).toBe("0");
    expect(zero.selfUsage?.partial).toBe(false);
    expect(zero.selfUsage?.requestCountSemantics).toBe("assistant_message");

    const codex = await usageApi.getAgentSessionUsage({
      appType: "codex",
      sessionId: "codex-usage-agent-call",
    });
    expect(codex.selfUsage?.cacheCreationTokens).toBeNull();
    expect(codex.selfUsage?.totalCostUsd).toBeNull();
    expect(codex.selfUsage?.partial).toBe(true);
    expect(codex.selfUsage?.requestCountSemantics).toBe("agent_call");

    const unavailable = await usageApi.getAgentSessionUsage({
      appType: "openclaw",
      sessionId: "openclaw-usage-unavailable",
    });
    expect(unavailable.totalUsage).toBeNull();
    expect(unavailable.precision).toBe("unavailable");
    expect(unavailable.partial).toBe(true);

    const capabilities = await usageApi.getAgentUsageCapabilities();
    expect(capabilities).toHaveLength(9);
    expect(capabilities.map(({ appType }) => appType)).toEqual([
      "claude",
      "claude-desktop",
      "codex",
      "gemini",
      "grokbuild",
      "opencode",
      "openclaw",
      "hermes",
      "pi",
    ]);
  });

  it("isolates session/range and task filter pagination query keys", () => {
    const noRange = normalizeAgentUsageRange();
    const week = normalizeAgentUsageRange({ startAt: 100, endAt: 200 });
    const month = normalizeAgentUsageRange({ startAt: 100, endAt: 300 });

    expect(usageKeys.agentSession("claude", "same", noRange)).not.toEqual(
      usageKeys.agentSession("codex", "same", noRange),
    );
    expect(usageKeys.agentSession("claude", "same", noRange)).not.toEqual(
      usageKeys.agentSession("claude", "other", noRange),
    );
    expect(usageKeys.agentSession("claude", "same", week)).not.toEqual(
      usageKeys.agentSession("claude", "same", month),
    );

    const firstPage = normalizeAgentTaskUsageFilter({
      appType: "claude",
      title: "first",
      project: "project-a",
      projectDir: "/mock/a",
      range: { startAt: 100, endAt: 200 },
      limit: 10,
      offset: 0,
    });
    const secondPage = normalizeAgentTaskUsageFilter({
      appType: "claude",
      title: "first",
      project: "project-a",
      projectDir: "/mock/a",
      range: { startAt: 100, endAt: 200 },
      limit: 10,
      offset: 10,
    });
    expect(usageKeys.agentTasks(firstPage)).not.toEqual(
      usageKeys.agentTasks(secondPage),
    );

    const changedFilter = normalizeAgentTaskUsageFilter({
      appType: "claude",
      title: "first",
      project: "project-a",
      projectDir: "/mock/b",
      range: { startAt: 100, endAt: 200 },
      limit: 10,
      offset: 0,
    });
    expect(usageKeys.agentTasks(firstPage)).not.toEqual(
      usageKeys.agentTasks(changedFilter),
    );

    const exactTitle = normalizeAgentTaskUsageFilter({
      appType: "claude",
      titleExact: "Build",
      projectDirExact: "/mock/a",
      range: { startAt: 100, endAt: 200 },
      limit: 10,
      offset: 0,
    });
    expect(usageKeys.agentTasks(firstPage)).not.toEqual(
      usageKeys.agentTasks(exactTitle),
    );

    expect(
      usageKeys.agentTaskFilterOptions("claude", week),
    ).not.toEqual(usageKeys.agentTaskFilterOptions("codex", week));
    expect(
      usageKeys.agentTaskFilterOptions("claude", week),
    ).not.toEqual(usageKeys.agentTaskFilterOptions("claude", month));
  });

  it("resolves moving task and filter-option ranges again on refetch", async () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date("2026-01-01T00:00:00.000Z"));
      const taskCalls: AgentTaskUsageFilter[] = [];
      const filterOptionCalls: AgentTaskUsageFilterOptionsRequest[] = [];
      vi.spyOn(usageApi, "listAgentTaskUsage").mockImplementation(
        async (filter) => {
          taskCalls.push(filter ?? {});
          return emptyTaskPage;
        },
      );
      vi.spyOn(usageApi, "getAgentTaskUsageFilterOptions").mockImplementation(
        async (request) => {
          filterOptionCalls.push(request ?? {});
          return filterOptions;
        },
      );

      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const selection = { preset: "today" as const };
      const task = renderHook(
        () =>
          useAgentTaskUsage(
            {
              appType: "claude",
              rangeSelection: selection,
              limit: 20,
              offset: 0,
            },
            { enabled: false },
          ),
        { wrapper: queryWrapper(client) },
      );
      const options = renderHook(
        () =>
          useAgentTaskUsageFilterOptions(
            { appType: "claude", rangeSelection: selection },
            { enabled: false },
          ),
        { wrapper: queryWrapper(client) },
      );

      await act(async () => {
        await Promise.all([
          task.result.current.refetch(),
          options.result.current.refetch(),
        ]);
      });
      expect(taskCalls).toHaveLength(1);
      expect(filterOptionCalls).toHaveLength(1);
      expect(taskCalls[0].range?.endAt).toBe(1767225600);
      expect(filterOptionCalls[0].range?.endAt).toBe(1767225600);
      expect(taskCalls[0]).not.toHaveProperty("rangeSelection");
      expect(filterOptionCalls[0]).not.toHaveProperty("rangeSelection");

      vi.advanceTimersByTime(4500);
      await act(async () => {
        await Promise.all([
          task.result.current.refetch(),
          options.result.current.refetch(),
        ]);
      });
      expect(taskCalls).toHaveLength(2);
      expect(filterOptionCalls).toHaveLength(2);
      expect(taskCalls[1].range?.endAt).toBeGreaterThan(
        taskCalls[0].range?.endAt ?? -1,
      );
      expect(filterOptionCalls[1].range?.endAt).toBeGreaterThan(
        filterOptionCalls[0].range?.endAt ?? -1,
      );

      // Re-normalizing after time moves does not create a timestamped key.
      const firstKey = usageKeys.agentTasks(
        normalizeAgentTaskUsageFilter({ rangeSelection: selection }),
      );
      const firstOptionsFilter = normalizeAgentTaskUsageFilter({
        rangeSelection: selection,
      });
      vi.advanceTimersByTime(20_000);
      const secondOptionsFilter = normalizeAgentTaskUsageFilter({
        rangeSelection: selection,
      });
      expect(firstKey).toEqual(usageKeys.agentTasks(secondOptionsFilter));
      expect(
        usageKeys.agentTaskFilterOptions(
          "claude",
          firstOptionsFilter.range,
          firstOptionsFilter.rangeSelection,
        ),
      ).toEqual(
        usageKeys.agentTaskFilterOptions(
          "claude",
          secondOptionsFilter.range,
          secondOptionsFilter.rangeSelection,
        ),
      );
    } finally {
      vi.useRealTimers();
      vi.restoreAllMocks();
    }
  });

  it("keeps fixed custom task ranges stable across refetches", async () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date("2026-01-01T00:00:00.000Z"));
      const taskCalls: AgentTaskUsageFilter[] = [];
      const filterOptionCalls: AgentTaskUsageFilterOptionsRequest[] = [];
      vi.spyOn(usageApi, "listAgentTaskUsage").mockImplementation(
        async (filter) => {
          taskCalls.push(filter ?? {});
          return emptyTaskPage;
        },
      );
      vi.spyOn(usageApi, "getAgentTaskUsageFilterOptions").mockImplementation(
        async (request) => {
          filterOptionCalls.push(request ?? {});
          return filterOptions;
        },
      );

      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const selection = {
        preset: "custom" as const,
        customStartDate: 100,
        customEndDate: 200,
      };
      const task = renderHook(
        () =>
          useAgentTaskUsage(
            { appType: "claude", rangeSelection: selection },
            { enabled: false },
          ),
        { wrapper: queryWrapper(client) },
      );
      const options = renderHook(
        () =>
          useAgentTaskUsageFilterOptions(
            { appType: "claude", rangeSelection: selection },
            { enabled: false },
          ),
        { wrapper: queryWrapper(client) },
      );

      await act(async () => {
        await Promise.all([
          task.result.current.refetch(),
          options.result.current.refetch(),
        ]);
      });
      vi.advanceTimersByTime(60_000);
      await act(async () => {
        await Promise.all([
          task.result.current.refetch(),
          options.result.current.refetch(),
        ]);
      });

      expect(taskCalls).toHaveLength(2);
      expect(filterOptionCalls).toHaveLength(2);
      expect(taskCalls[0].range).toEqual({ startAt: 100, endAt: 200 });
      expect(taskCalls[1].range).toEqual(taskCalls[0].range);
      expect(filterOptionCalls[0].range).toEqual({
        startAt: 100,
        endAt: 200,
      });
      expect(filterOptionCalls[1].range).toEqual(filterOptionCalls[0].range);
    } finally {
      vi.useRealTimers();
      vi.restoreAllMocks();
    }
  });
});
