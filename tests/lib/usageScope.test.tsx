import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  useUsageSummary,
  useUsageSummaryByApp,
  useUsageTrends,
  useUsageFirstDate,
  useProviderStats,
  useModelStats,
  useProjectStats,
  useUsageProjects,
  useUsageDataQuality,
  useRequestLogs,
} from "@/lib/query/usage";
import type { UsageRangeSelection, UsageScopeFilters } from "@/types/usage";

const api = vi.hoisted(() => ({
  getUsageSummary: vi.fn(),
  getUsageSummaryByApp: vi.fn(),
  getUsageTrends: vi.fn(),
  getUsageFirstDate: vi.fn(),
  getProviderStats: vi.fn(),
  getModelStats: vi.fn(),
  getProjectStats: vi.fn(),
  getUsageProjects: vi.fn(),
  getUsageDataQuality: vi.fn(),
  getRequestLogs: vi.fn(),
}));
vi.mock("@/lib/api/usage", () => ({ usageApi: api }));
const clients: QueryClient[] = [];
function wrapper() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  clients.push(client);
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
}
const range: UsageRangeSelection = {
  preset: "custom",
  customStartDate: 1000,
  customEndDate: 2000,
};
const scope: UsageScopeFilters = {
  appType: "claude",
  providerAppType: "claude",
  providerId: "p",
  providerName: "Provider",
  model: "model-x",
  projectKeys: ["project-b", "project-a", "project-b"],
};
const options = { refetchInterval: false as const };
const args = [
  1000,
  2000,
  "claude",
  "claude",
  "p",
  "Provider",
  "model-x",
  ["project-a", "project-b"],
];

describe("usage scope query contract", () => {
  beforeEach(() => {
    for (const query of Object.values(api))
      query.mockReset().mockResolvedValue([]);
  });
  afterEach(() => {
    clients.splice(0).forEach((client) => client.clear());
  });

  const queries: Array<{
    name: keyof typeof api;
    hook: () => { isSuccess: boolean };
    expected: unknown[];
  }> = [
    {
      name: "getUsageSummary",
      hook: () => useUsageSummary(range, scope, options),
      expected: args,
    },
    {
      name: "getUsageSummaryByApp",
      hook: () => useUsageSummaryByApp(range, scope, options),
      expected: [
        1000,
        2000,
        "claude",
        "p",
        "Provider",
        "model-x",
        ["project-a", "project-b"],
      ],
    },
    {
      name: "getUsageTrends",
      hook: () => useUsageTrends(range, scope, options),
      expected: args,
    },
    {
      name: "getUsageFirstDate",
      hook: () => useUsageFirstDate(scope, options),
      expected: args.slice(2),
    },
    {
      name: "getProviderStats",
      hook: () => useProviderStats(range, scope, options),
      expected: args,
    },
    {
      name: "getModelStats",
      hook: () => useModelStats(range, scope, options),
      expected: args,
    },
    {
      name: "getProjectStats",
      hook: () => useProjectStats(range, scope, options),
      expected: args,
    },
    {
      name: "getUsageDataQuality",
      hook: () => useUsageDataQuality(range, scope, options),
      expected: args,
    },
    {
      name: "getUsageProjects",
      hook: () => useUsageProjects(range, scope, options),
      expected: args.slice(0, 7),
    },
  ];
  it.each(queries)(
    "$name forwards the complete normalized scope",
    async ({ name, hook, expected }) => {
      const { result } = renderHook(hook, { wrapper: wrapper() });
      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(api[name]).toHaveBeenLastCalledWith(...expected);
    },
  );

  it("normalizes project set order and invalidates the cache only when membership changes", async () => {
    const { result, rerender } = renderHook(
      ({ projectKeys }) => useProjectStats(range, { projectKeys }, options),
      { wrapper: wrapper(), initialProps: { projectKeys: ["b", "a", "b"] } },
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    rerender({ projectKeys: ["a", "b"] });
    expect(api.getProjectStats).toHaveBeenCalledTimes(1);
    rerender({ projectKeys: ["a", "c"] });
    await waitFor(() => expect(api.getProjectStats).toHaveBeenCalledTimes(2));
    expect(api.getProjectStats.mock.lastCall?.[7]).toEqual(["a", "c"]);
  });

  it.each([
    { selected: [], expected: undefined },
    { selected: [""], expected: [""] },
  ])(
    "keeps all projects and unidentified projects distinct ($selected)",
    async ({ selected, expected }) => {
      const { result } = renderHook(
        () => useUsageDataQuality(range, { projectKeys: selected }, options),
        { wrapper: wrapper() },
      );
      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(api.getUsageDataQuality.mock.lastCall?.[7]).toEqual(expected);
    },
  );

  it("keeps the project facet independent of its own selected values", async () => {
    const { result, rerender } = renderHook(
      ({ projectKeys }) =>
        useUsageProjects(range, { ...scope, projectKeys }, options),
      { wrapper: wrapper(), initialProps: { projectKeys: ["a"] } },
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    await act(async () => rerender({ projectKeys: ["b"] }));
    expect(api.getUsageProjects).toHaveBeenCalledTimes(1);
    expect(api.getUsageProjects).toHaveBeenLastCalledWith(
      1000,
      2000,
      "claude",
      "claude",
      "p",
      "Provider",
      "model-x",
    );
  });

  it("keys log counts and rows by the selected projects and local status", async () => {
    const { result, rerender } = renderHook(
      ({ projectKeys }) =>
        useRequestLogs({
          range,
          filters: { ...scope, projectKeys, statusCode: 500 },
          options,
        }),
      { wrapper: wrapper(), initialProps: { projectKeys: ["a"] } },
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    rerender({ projectKeys: ["b"] });
    await waitFor(() => expect(api.getRequestLogs).toHaveBeenCalledTimes(2));
    expect(api.getRequestLogs).toHaveBeenLastCalledWith(
      {
        appType: "claude",
        providerAppType: "claude",
        providerId: "p",
        providerName: "Provider",
        model: "model-x",
        projectKey: undefined,
        projectKeys: ["b"],
        statusCode: 500,
        startDate: 1000,
        endDate: 2000,
      },
      0,
      20,
    );
  });
});
