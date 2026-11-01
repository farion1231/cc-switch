import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BreakerEvidencePanel } from "@/components/providers/BreakerEvidencePanel";
import type { CircuitBreakerStats } from "@/types/proxy";
import { createTestQueryClient } from "../utils/testQueryClient";

const statsRef = vi.hoisted(() => ({
  value: undefined as CircuitBreakerStats | undefined,
}));
const resetMutate = vi.hoisted(() => vi.fn());

vi.mock("@/lib/query/failover", () => ({
  useCircuitBreakerStats: () => ({ data: statsRef.value }),
  useResetCircuitBreaker: () => ({
    mutate: resetMutate,
    isPending: false,
  }),
}));

function renderPanel() {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <BreakerEvidencePanel providerId="p1" appType="claude" />
    </QueryClientProvider>,
  );
}

function stats(
  overrides: Partial<CircuitBreakerStats> = {},
): CircuitBreakerStats {
  return {
    state: "closed",
    consecutiveFailures: 0,
    consecutiveSuccesses: 0,
    totalRequests: 0,
    failedRequests: 0,
    failureEvidence: [],
    ...overrides,
  };
}

beforeEach(() => {
  statsRef.value = undefined;
  resetMutate.mockClear();
});

describe("BreakerEvidencePanel", () => {
  it("renders nothing when the provider has no breaker instance yet", () => {
    // 从没失败过的供应商不该在卡片上多占一块地方
    const { container } = renderPanel();
    expect(container).toBeEmptyDOMElement();
  });

  it("summarises the evidence count and the consecutive failures", () => {
    statsRef.value = stats({
      consecutiveFailures: 3,
      failureEvidence: [
        {
          at: "2026-10-30T10:00:03Z",
          status: 503,
          reason: "overloaded",
          tripped: true,
        },
        {
          at: "2026-10-30T10:00:02Z",
          status: 503,
          reason: "overloaded",
          tripped: false,
        },
        {
          at: "2026-10-30T10:00:01Z",
          status: 503,
          reason: "overloaded",
          tripped: false,
        },
      ],
    });
    renderPanel();

    expect(screen.getByText(/失败证据 3 条/)).toBeInTheDocument();
    expect(screen.getByText(/连续失败 3 次/)).toBeInTheDocument();
  });

  it("lists every piece of evidence only after the user expands it", () => {
    statsRef.value = stats({
      consecutiveFailures: 2,
      failureEvidence: [
        {
          at: "2026-10-30T10:00:02Z",
          status: 500,
          reason: "first boom",
          tripped: false,
        },
        {
          at: "2026-10-30T10:00:01Z",
          status: 502,
          reason: "second boom",
          tripped: true,
        },
      ],
    });
    renderPanel();

    // 收起时只给结论，不把一屏错误铺在卡片上
    expect(screen.queryByText("first boom")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /失败证据/ }));

    expect(screen.getByText("first boom")).toBeInTheDocument();
    expect(screen.getByText("second boom")).toBeInTheDocument();
  });

  it("marks exactly the request that tripped the breaker", () => {
    statsRef.value = stats({
      state: "open",
      consecutiveFailures: 2,
      failureEvidence: [
        {
          at: "2026-10-30T10:00:02Z",
          status: 500,
          reason: "the one that did it",
          tripped: true,
        },
        {
          at: "2026-10-30T10:00:01Z",
          status: 500,
          reason: "an earlier one",
          tripped: false,
        },
      ],
    });
    renderPanel();
    fireEvent.click(screen.getByRole("button", { name: /失败证据/ }));

    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(2);
    // 新的在前
    expect(
      within(items[0]).getByText("the one that did it"),
    ).toBeInTheDocument();
    expect(within(items[0]).getByText("触发熔断")).toBeInTheDocument();
    expect(within(items[1]).queryByText("触发熔断")).not.toBeInTheDocument();
  });

  it("labels a network-layer failure instead of inventing a status code", () => {
    statsRef.value = stats({
      consecutiveFailures: 1,
      failureEvidence: [
        {
          at: "2026-10-30T10:00:00Z",
          status: null,
          reason: "请求超时",
          tripped: false,
        },
      ],
    });
    renderPanel();
    fireEvent.click(screen.getByRole("button", { name: /失败证据/ }));

    const item = screen.getByRole("listitem");
    // 显示 0 会被读成「HTTP 0」，那是个不存在的状态码
    expect(within(item).getByText("网络")).toBeInTheDocument();
    expect(within(item).queryByText("0")).not.toBeInTheDocument();
  });

  it("says the probe failed when the breaker is half-open", () => {
    statsRef.value = stats({
      state: "half_open",
      consecutiveFailures: 1,
      failureEvidence: [
        {
          at: "2026-10-30T10:00:00Z",
          status: 503,
          reason: "still down",
          tripped: true,
        },
      ],
    });
    renderPanel();
    fireEvent.click(screen.getByRole("button", { name: /失败证据/ }));

    expect(screen.getByText("探测失败")).toBeInTheDocument();
    expect(screen.queryByText("触发熔断")).not.toBeInTheDocument();
  });

  it("clears the counters for this provider and app", () => {
    statsRef.value = stats({
      state: "open",
      consecutiveFailures: 5,
      failureEvidence: [
        {
          at: "2026-10-30T10:00:00Z",
          status: 500,
          reason: "boom",
          tripped: true,
        },
      ],
    });
    renderPanel();

    fireEvent.click(screen.getByRole("button", { name: /清除计数/ }));

    expect(resetMutate).toHaveBeenCalledWith({
      providerId: "p1",
      appType: "claude",
    });
  });

  it("renders nothing for a healthy provider with no failure history", () => {
    // 熔断器是按故障转移队列懒建的：健康供应商也有实例，只是 0 失败 0 证据。
    // 空面板 + 毁灭观感的清除按钮只会给每张卡添噪音（审计 m1）。
    statsRef.value = stats({ consecutiveFailures: 0, failureEvidence: [] });
    const { container } = renderPanel();

    expect(container).toBeEmptyDOMElement();
  });

  it("shows the evidence summary even when the breaker is closed again", () => {
    // 恢复后连续失败归零，但证据还在——它记录的是发生过什么
    statsRef.value = stats({
      state: "closed",
      consecutiveFailures: 0,
      failureEvidence: [
        {
          at: "2026-10-30T10:00:00Z",
          status: 503,
          reason: "already recovered",
          tripped: true,
        },
      ],
    });
    renderPanel();

    expect(screen.getByText(/失败证据 1 条/)).toBeInTheDocument();
    expect(screen.getByText(/连续失败 0 次/)).toBeInTheDocument();
  });
});
