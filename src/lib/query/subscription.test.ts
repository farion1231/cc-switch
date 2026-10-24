import { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CodexResetsStatus } from "@/types/subscription";
import { subscriptionApi } from "@/lib/api/subscription";
import { forceRefreshCodexResets, subscriptionKeys } from "./subscription";

vi.mock("@/lib/api/subscription", () => ({
  subscriptionApi: { getCodexResetsStatus: vi.fn() },
}));

const status: CodexResetsStatus = {
  latestReset: null,
  scheduledReset: null,
  activeWatch: null,
  stats: {
    total: 0,
    lastResetAt: null,
    daysSinceLast: null,
    avgIntervalDays: null,
  },
  fetchedAt: 0,
};

describe("forceRefreshCodexResets", () => {
  beforeEach(() => {
    vi.mocked(subscriptionApi.getCodexResetsStatus).mockReset();
  });

  it("开关关闭时跳过，不发请求也不动缓存", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(["settings"], { codexResetPrediction: false });

    await forceRefreshCodexResets(queryClient);

    expect(subscriptionApi.getCodexResetsStatus).not.toHaveBeenCalled();
    expect(queryClient.getQueryData(subscriptionKeys.codexResets())).toBeUndefined();
  });

  it("设置还没读到时同样跳过（功能默认关）", async () => {
    const queryClient = new QueryClient();

    await forceRefreshCodexResets(queryClient);

    expect(subscriptionApi.getCodexResetsStatus).not.toHaveBeenCalled();
  });

  it("开关打开时强制刷新并写回共享缓存", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(["settings"], { codexResetPrediction: true });
    vi.mocked(subscriptionApi.getCodexResetsStatus).mockResolvedValue(status);

    await forceRefreshCodexResets(queryClient);

    expect(subscriptionApi.getCodexResetsStatus).toHaveBeenCalledWith(true);
    expect(
      queryClient.getQueryData(subscriptionKeys.codexResets()),
    ).toEqual(status);
  });
});
