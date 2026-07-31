import { createElement, type ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { getUsageSummaryMock } = vi.hoisted(() => ({
  getUsageSummaryMock: vi.fn(),
}));

const { getProvidersMock, getCurrentProviderMock, queryUsageMock } = vi.hoisted(
  () => ({
    getProvidersMock: vi.fn(),
    getCurrentProviderMock: vi.fn(),
    queryUsageMock: vi.fn(),
  }),
);

vi.mock("@/lib/api/usage", () => ({
  usageApi: {
    getUsageSummary: getUsageSummaryMock,
  },
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    getAll: getProvidersMock,
    getCurrent: getCurrentProviderMock,
  },
  usageApi: {
    query: queryUsageMock,
  },
}));

import { useUsageSummary, usageKeys } from "@/lib/query/usage";
import { modelsDevSyncConfigQueryKey } from "@/lib/modelsDevAutoSync";
import {
  providerKeys,
  useProvidersQuery,
  useUsageQuery,
} from "@/lib/query/queries";
import { runtimeQueryScope, useRuntimeQueryScope } from "./queryScope";
import { setRuntimeSnapshot } from "./store";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

describe("runtime query scope", () => {
  beforeEach(() => {
    getUsageSummaryMock.mockReset();
    getUsageSummaryMock.mockResolvedValue({ totalRequests: 0 });
    getProvidersMock.mockReset();
    getCurrentProviderMock.mockReset();
    queryUsageMock.mockReset();
    setRuntimeSnapshot({ status: "local", generation: 0 });
  });

  it("creates distinct scopes for local, remote and transition generations", () => {
    expect(runtimeQueryScope({ status: "local", generation: 2 })).toEqual([
      "local",
      2,
    ]);
    expect(
      runtimeQueryScope({
        status: "online",
        generation: 3,
        activeTargetId: "server-a",
      }),
    ).toEqual(["remote", "server-a", 3]);
    expect(
      runtimeQueryScope({
        status: "connecting",
        generation: 4,
        activeTargetId: "server-b",
      }),
    ).toEqual(["transition", "server-b", 4]);
  });

  it("places the runtime scope immediately after each domain root", () => {
    const scope = ["remote", "server-a", 7] as const;

    expect(usageKeys.all(scope)).toEqual(["usage", ...scope]);
    expect(usageKeys.script("provider-a", "codex", scope)).toEqual([
      "usage",
      ...scope,
      "script",
      "provider-a",
      "codex",
    ]);
    expect(providerKeys.byApp("codex", scope)).toEqual([
      "providers",
      ...scope,
      "codex",
    ]);
    expect(modelsDevSyncConfigQueryKey(scope)).toEqual([
      "models-dev-sync-config",
      ...scope,
    ]);
  });

  it("refetches a mounted Usage hook under a new key after target switch", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);

    const { result } = renderHook(
      () => {
        const scope = useRuntimeQueryScope();
        const query = useUsageSummary({ preset: "today" }, undefined, {
          refetchInterval: false,
        });
        return { scope, query };
      },
      { wrapper },
    );

    await waitFor(() => expect(getUsageSummaryMock).toHaveBeenCalledTimes(1));
    expect(result.current.scope).toEqual(["local", 0]);

    act(() => {
      setRuntimeSnapshot({
        status: "online",
        generation: 1,
        activeTargetId: "server-a",
      });
    });

    await waitFor(() => expect(getUsageSummaryMock).toHaveBeenCalledTimes(2));
    expect(result.current.scope).toEqual(["remote", "server-a", 1]);
    expect(
      queryClient.getQueryCache().findAll({ queryKey: ["usage"] }),
    ).toHaveLength(2);
  });

  it("does not expose previous-host Provider data while the new host loads", async () => {
    const remoteProviders = deferred<Record<string, never>>();
    getProvidersMock
      .mockResolvedValueOnce({
        local: { id: "local", name: "Local", settingsConfig: {} },
      })
      .mockReturnValueOnce(remoteProviders.promise);
    getCurrentProviderMock.mockResolvedValue("local");
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);

    const { result } = renderHook(() => useProvidersQuery("codex"), {
      wrapper,
    });
    await waitFor(() =>
      expect(result.current.data?.providers.local?.name).toBe("Local"),
    );

    act(() => {
      setRuntimeSnapshot({
        status: "online",
        generation: 1,
        activeTargetId: "server-a",
      });
    });

    await waitFor(() => expect(result.current.isFetching).toBe(true));
    expect(result.current.data).toBeUndefined();

    await act(async () => remoteProviders.resolve({}));
  });

  it("drops keep-last-good Usage data when runtime scope changes", async () => {
    queryUsageMock
      .mockResolvedValueOnce({ success: true, used: 10 })
      .mockResolvedValueOnce({ success: false, error: "HTTP 500" });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);

    const { result } = renderHook(() => useUsageQuery("provider-a", "codex"), {
      wrapper,
    });
    await waitFor(() => expect(result.current.data?.success).toBe(true));

    act(() => {
      setRuntimeSnapshot({
        status: "online",
        generation: 1,
        activeTargetId: "server-a",
      });
    });

    await waitFor(() => expect(queryUsageMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(result.current.isFetching).toBe(false));
    expect(result.current.data).toMatchObject({
      success: false,
      error: "HTTP 500",
    });
  });
});
