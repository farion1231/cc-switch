import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RequestLogTable } from "@/components/usage/RequestLogTable";
import type { UsageRangeSelection } from "@/types/usage";

const useRequestLogsMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (
      key: string,
      options?: {
        defaultValue?: string;
      },
    ) => options?.defaultValue ?? key,
    i18n: {
      resolvedLanguage: "en",
      language: "en",
    },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useRequestLogs: (args: unknown) => useRequestLogsMock(args),
}));

describe("RequestLogTable", () => {
  beforeEach(() => {
    useRequestLogsMock.mockReset();
    useRequestLogsMock.mockImplementation(
      ({ page = 0, pageSize = 20 }: { page?: number; pageSize?: number }) => ({
        data: {
          data: [],
          total: 120,
          page,
          pageSize,
        },
        isLoading: false,
      }),
    );
  });

  it("resets pagination when the dashboard range changes", async () => {
    const initialRange: UsageRangeSelection = { preset: "today" };
    const nextRange: UsageRangeSelection = {
      preset: "custom",
      customStartDate: 1_710_000_000,
      customEndDate: 1_710_086_400,
    };

    const { rerender } = render(
      <RequestLogTable
        range={initialRange}
        rangeLabel="Today"
        appType="all"
        refreshIntervalMs={0}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "usage.nextPage" }));

    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({
          page: 1,
          range: initialRange,
        }),
      );
    });

    rerender(
      <RequestLogTable
        range={nextRange}
        rangeLabel="Custom"
        appType="all"
        refreshIntervalMs={0}
      />,
    );

    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({
          page: 0,
          range: nextRange,
        }),
      );
    });
  });

  it("resets pagination when the dashboard app filter changes", async () => {
    const range: UsageRangeSelection = { preset: "today" };
    const { rerender } = render(
      <RequestLogTable
        range={range}
        rangeLabel="Today"
        appType="all"
        refreshIntervalMs={0}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "usage.nextPage" }));

    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({
          page: 1,
          range,
        }),
      );
    });

    rerender(
      <RequestLogTable
        range={range}
        rangeLabel="Today"
        appType="claude"
        refreshIntervalMs={0}
      />,
    );

    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({
          page: 0,
          range,
        }),
      );
    });
  });

  it("shows speed only for routed requests with at least 100 output tokens", () => {
    const base = {
      providerId: "p1",
      providerName: "DeepSeek",
      appType: "codex",
      model: "deepseek-v4-pro",
      costMultiplier: "1",
      inputTokens: 1_000,
      cacheReadTokens: 0,
      cacheCreationTokens: 0,
      inputCostUsd: "0",
      outputCostUsd: "0",
      cacheReadCostUsd: "0",
      cacheCreationCostUsd: "0",
      totalCostUsd: "0.0114",
      isStreaming: true,
      statusCode: 200,
      createdAt: 1_759_212_000,
    };
    useRequestLogsMock.mockReturnValue({
      data: {
        data: [
          // 1100 token / (12.9 - 1.8) s ≈ 99 tok/s
          {
            ...base,
            requestId: "fast",
            outputTokens: 1_100,
            latencyMs: 12_900,
            firstTokenMs: 1_800,
          },
          // 输出不到 100：不算
          {
            ...base,
            requestId: "short",
            outputTokens: 64,
            latencyMs: 2_400,
            firstTokenMs: 1_900,
          },
          // 会话日志：没有首字
          {
            ...base,
            requestId: "session",
            outputTokens: 900,
            latencyMs: 0,
            dataSource: "codex_session",
          },
        ],
        total: 3,
        page: 0,
        pageSize: 20,
      },
      isLoading: false,
    });
    const onOpenDetail = vi.fn();

    render(
      <RequestLogTable
        range={{ preset: "7d" }}
        refreshIntervalMs={0}
        onOpenDetail={onOpenDetail}
      />,
    );

    const rows = screen.getAllByRole("row").slice(1);
    expect(rows).toHaveLength(3);
    expect(rows[0].lastElementChild).toHaveTextContent("99tok/s");
    expect(rows[0].lastElementChild).toHaveAttribute(
      "title",
      "usage.timingTip",
    );
    expect(rows[1].lastElementChild).toHaveTextContent("—");
    expect(rows[2].lastElementChild).toHaveTextContent("—");
    expect(
      screen.getByRole("columnheader", { name: /usage.speed/ }),
    ).toBeInTheDocument();

    fireEvent.click(rows[1]);
    expect(onOpenDetail).toHaveBeenCalledWith("short");
  });
});
