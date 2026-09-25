import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { server } from "../msw/server";
import { setLiveProviderIds } from "../msw/state";
import { useDevEcoLiveProviderIds } from "@/hooks/useDevEco";
import { useSwitchProviderMutation } from "@/lib/query/mutations";

const TAURI_ENDPOINT = "http://tauri.local";

const createWrapper = (queryClient: QueryClient) =>
  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };

const createQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });

describe("DevEco live provider id cache", () => {
  beforeEach(() => {
    setLiveProviderIds("deveco", []);
    server.use(
      http.post(`${TAURI_ENDPOINT}/switch_provider`, () =>
        HttpResponse.json(true),
      ),
      http.post(`${TAURI_ENDPOINT}/update_tray_menu`, () =>
        HttpResponse.json(true),
      ),
    );
  });

  /**
   * liveProviderIds 是独立于 providers 列表的查询，且没有轮询：供应商操作完成后
   * 不失效它，卡片就会一直停在旧状态（添加后仍显示未添加、移出后仍显示在配置中），
   * 直到重新挂载。切页或刷 providers 都救不回来。
   */
  it("refetches live ids after switching a DevEco provider", async () => {
    const queryClient = createQueryClient();
    const wrapper = createWrapper(queryClient);

    const { result } = renderHook(
      () => ({
        live: useDevEcoLiveProviderIds(true),
        switchProvider: useSwitchProviderMutation("deveco"),
      }),
      { wrapper },
    );

    await waitFor(() => expect(result.current.live.isSuccess).toBe(true));
    expect(result.current.live.data).toEqual([]);
    expect(result.current.live.dataUpdatedAt).toBeGreaterThan(0);
    const firstFetch = result.current.live.dataUpdatedAt;

    // 原生配置里已经有了这个供应商，缓存仍是空 —— 正是需要失效的场景。
    setLiveProviderIds("deveco", ["p"]);

    await result.current.switchProvider.mutateAsync("p");

    await waitFor(() => expect(result.current.live.data).toEqual(["p"]));
    expect(result.current.live.dataUpdatedAt).toBeGreaterThan(firstFetch);
  });
});
