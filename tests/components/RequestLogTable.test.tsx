import { fireEvent, screen, waitFor } from "@testing-library/react";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  RequestLogTable,
  appShortName,
  formatHitPercent,
  formatLogTime,
} from "@/components/usage/RequestLogTable";
import { REQUEST_LOG_COLUMNS } from "@/components/usage/requestLogColumns";
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

  it("pulls the page back into range when the total shrinks", async () => {
    let total = 120;
    useRequestLogsMock.mockImplementation(
      ({ page = 0, pageSize = 20 }: { page?: number; pageSize?: number }) => ({
        data: { data: [], total, page, pageSize },
        isLoading: false,
        isPlaceholderData: false,
      }),
    );
    const range: UsageRangeSelection = { preset: "today" };
    const table = (
      <RequestLogTable range={range} appType="all" refreshIntervalMs={0} />
    );
    const { rerender } = render(table);

    fireEvent.click(screen.getByRole("button", { name: "usage.nextPage" }));
    fireEvent.click(screen.getByRole("button", { name: "usage.nextPage" }));
    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({ page: 2 }),
      );
    });

    // 「今天」跨日后只剩 5 条：只有一页、翻页条不画，必须自己回到第一页
    total = 5;
    rerender(
      <RequestLogTable range={range} appType="all" refreshIntervalMs={0} />,
    );
    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({ page: 0 }),
      );
    });
    expect(
      screen.queryByRole("button", { name: "usage.nextPage" }),
    ).not.toBeInTheDocument();

    // 还剩几页时收回到最后一页，而不是第一页
    total = 120;
    rerender(table);
    fireEvent.click(screen.getByRole("button", { name: "usage.nextPage" }));
    fireEvent.click(screen.getByRole("button", { name: "usage.nextPage" }));
    fireEvent.click(screen.getByRole("button", { name: "usage.nextPage" }));
    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({ page: 3 }),
      );
    });
    total = 50;
    rerender(
      <RequestLogTable range={range} appType="all" refreshIntervalMs={0} />,
    );
    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({ page: 2 }),
      );
    });
  });

  it("shows exact speed for routed requests and an estimate for timed session logs", () => {
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
          // 会话日志：没有首字，也没估出耗时
          {
            ...base,
            requestId: "session",
            outputTokens: 900,
            latencyMs: 0,
            dataSource: "codex_session",
          },
          // 会话日志：按估算耗时算，900 token / 10 s = 90 tok/s，带 ≈
          {
            ...base,
            requestId: "session-estimated",
            outputTokens: 900,
            latencyMs: 10_000,
            dataSource: "codex_session",
          },
          // 会话日志推出了首字：首字带 ≈，速度仍按整段耗时估
          {
            ...base,
            requestId: "session-first-token",
            outputTokens: 900,
            latencyMs: 10_000,
            firstTokenMs: 3_300,
            dataSource: "codex_session",
          },
          // Pi 用自己的请求开始/回复写入时间估算耗时，没有首字计时。
          {
            ...base,
            appType: "pi",
            requestId: "pi-session-estimated",
            outputTokens: 900,
            latencyMs: 10_000,
            dataSource: "pi_session",
          },
        ],
        total: 6,
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
    expect(rows).toHaveLength(6);
    // 最后两列：速度、首字
    const speedCell = (row: HTMLElement) =>
      row.children[row.children.length - 2];
    const firstTokenCell = (row: HTMLElement) => row.lastElementChild;
    expect(speedCell(rows[0])).toHaveTextContent("99tok/s");
    expect(speedCell(rows[0])).toHaveAttribute("title", "usage.timingTip");
    expect(firstTokenCell(rows[0])).toHaveTextContent("1.8s");
    expect(speedCell(rows[1])).toHaveTextContent("—");
    // 输出太少不算速度，但首字照样显示
    expect(firstTokenCell(rows[1])).toHaveTextContent("1.9s");
    expect(speedCell(rows[2])).toHaveTextContent("—");
    expect(firstTokenCell(rows[2])).toHaveTextContent("—");
    expect(speedCell(rows[3])).toHaveTextContent("≈90tok/s");
    expect(speedCell(rows[3])).toHaveAttribute(
      "title",
      "usage.estimatedTimingTip",
    );
    expect(firstTokenCell(rows[3])).toHaveTextContent("—");
    expect(speedCell(rows[4])).toHaveTextContent("≈90tok/s");
    expect(firstTokenCell(rows[4])).toHaveTextContent("≈3.3s");
    expect(firstTokenCell(rows[4])).toHaveAttribute(
      "title",
      "usage.estimatedTimingTipWithFirstToken",
    );
    expect(speedCell(rows[5])).toHaveTextContent("≈90tok/s");
    expect(speedCell(rows[5])).toHaveAttribute(
      "title",
      "usage.estimatedTimingTip",
    );
    expect(firstTokenCell(rows[5])).toHaveTextContent("—");
    expect(
      screen.getByRole("columnheader", { name: /usage.firstToken/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: /usage.speed/ }),
    ).toBeInTheDocument();

    fireEvent.click(rows[1]);
    expect(onOpenDetail).toHaveBeenCalledWith("short");
  });

  it("shows full provider names on hover and short app names in the app column", () => {
    useRequestLogsMock.mockReturnValue({
      data: {
        data: [
          {
            requestId: "r1",
            providerId: "p1",
            providerName: "Kimi For Coding Plan Provider",
            appType: "claude",
            model: "kimi-k2.6",
            costMultiplier: "1",
            inputTokens: 10,
            outputTokens: 10,
            cacheReadTokens: 0,
            cacheCreationTokens: 0,
            inputCostUsd: "0",
            outputCostUsd: "0",
            cacheReadCostUsd: "0",
            cacheCreationCostUsd: "0",
            totalCostUsd: "0",
            isStreaming: false,
            statusCode: 200,
            latencyMs: 0,
            createdAt: Math.floor(Date.now() / 1000),
          },
        ],
        total: 1,
        page: 0,
        pageSize: 20,
      },
      isLoading: false,
    });

    render(
      <RequestLogTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );

    expect(
      screen.getByTitle("Kimi For Coding Plan Provider"),
    ).toHaveTextContent("Kimi For Coding Plan Provider");
    // 应用列：短名 + 全名在悬停提示和读屏文字里
    const appCell = screen.getByTitle("Claude Code");
    expect(appCell).toHaveTextContent("Claude");
    expect(appShortName("claude-desktop")).toBe("Desktop");
    expect(appShortName("unknown-app")).toBe("unknown-app");
  });
});

describe("RequestLogTable optional columns", () => {
  const base = {
    providerId: "p1",
    providerName: "DeepSeek",
    model: "m",
    costMultiplier: "1",
    outputTokens: 10,
    inputCostUsd: "0",
    outputCostUsd: "0",
    cacheReadCostUsd: "0",
    cacheCreationCostUsd: "0",
    totalCostUsd: "0",
    isStreaming: true,
    createdAt: Math.floor(Date.now() / 1000),
  };
  const allColumns = REQUEST_LOG_COLUMNS.map((column) => column.id);

  const cellsOf = (requestId: string) => {
    const row = screen
      .getAllByRole("row")
      .find((r) => r.querySelector(`[data-request="${requestId}"]`));
    if (!row) throw new Error(`row ${requestId} not found`);
    const headers = screen
      .getAllByRole("columnheader")
      .map((th) => th.textContent ?? "");
    const cells = Array.from(row.querySelectorAll("td"));
    // 带「?」说明的列头文字后面跟着说明按钮
    return (header: string) =>
      cells[headers.findIndex((text) => text.startsWith(header))];
  };

  beforeEach(() => {
    useRequestLogsMock.mockReset();
  });

  it("shows only the default columns unless told otherwise", () => {
    useRequestLogsMock.mockReturnValue({
      data: { data: [], total: 0, page: 0, pageSize: 20 },
      isLoading: false,
    });
    render(
      <RequestLogTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );
    const headers = screen
      .getAllByRole("columnheader")
      .map((th) => th.textContent);
    expect(headers).toHaveLength(10);
    expect(headers).not.toContain("usage.logColumns.hitRate");
    expect(screen.getByText("usage.noData")).toHaveAttribute("colspan", "10");
  });

  it("renders cache write, hit rate, total time and status code", () => {
    useRequestLogsMock.mockReturnValue({
      data: {
        data: [
          // Claude：新增输入 200 + 写入 800 + 命中 9000 → 90.0%
          {
            ...base,
            requestId: "claude",
            appType: "claude",
            inputTokens: 200,
            cacheCreationTokens: 800,
            cacheReadTokens: 9_000,
            latencyMs: 12_340,
            statusCode: 200,
          },
          // Codex：输入含命中；不报缓存写入
          {
            ...base,
            requestId: "codex",
            appType: "codex",
            inputTokens: 1_000,
            cacheCreationTokens: 0,
            cacheReadTokens: 999,
            latencyMs: 4_000,
            dataSource: "codex_session",
            statusCode: 200,
          },
          // 失败请求：没有输入，也没有耗时
          {
            ...base,
            requestId: "failed",
            appType: "claude",
            inputTokens: 0,
            cacheCreationTokens: 0,
            cacheReadTokens: 0,
            latencyMs: 0,
            statusCode: 429,
            errorMessage: "rate limited",
          },
        ].map((log) => ({ ...log, model: log.requestId })),
        total: 3,
        page: 0,
        pageSize: 20,
      },
      isLoading: false,
    });

    render(
      <RequestLogTable
        range={{ preset: "today" }}
        refreshIntervalMs={0}
        columns={allColumns}
      />,
    );
    // 给每行打个标记，按列头找单元格
    for (const id of ["claude", "codex", "failed"]) {
      screen.getByTitle(id).setAttribute("data-request", id);
    }

    const claude = cellsOf("claude");
    expect(claude("usage.logColumns.cacheWrite")).toHaveTextContent("800");
    expect(claude("usage.logColumns.hitRate")).toHaveTextContent("90.0%");
    expect(claude("usage.logColumns.duration")).toHaveTextContent("12.3s");
    expect(claude("usage.statusCode")).toHaveTextContent("200");

    const codex = cellsOf("codex");
    expect(codex("usage.logColumns.cacheWrite")).toHaveTextContent("—");
    expect(codex("usage.logColumns.cacheWrite")).toHaveAttribute(
      "title",
      "usage.cacheWriteNotReported",
    );
    // 999 / 1000 不进成 100%
    expect(codex("usage.logColumns.hitRate")).toHaveTextContent("99.9%");
    expect(codex("usage.logColumns.duration")).toHaveTextContent("≈4.0s");

    const failed = cellsOf("failed");
    expect(failed("usage.logColumns.hitRate")).toHaveTextContent("—");
    expect(failed("usage.logColumns.duration")).toHaveTextContent("—");
    expect(failed("usage.statusCode")).toHaveTextContent("429");
    // 有状态码列时，时间旁边不再重复挂错误码
    expect(failed("usage.time")).not.toHaveTextContent("429");
  });

  it("keeps the error code next to the time when the status column is off", () => {
    useRequestLogsMock.mockReturnValue({
      data: {
        data: [
          {
            ...base,
            requestId: "failed",
            appType: "claude",
            inputTokens: 0,
            cacheCreationTokens: 0,
            cacheReadTokens: 0,
            latencyMs: 0,
            statusCode: 500,
          },
        ],
        total: 1,
        page: 0,
        pageSize: 20,
      },
      isLoading: false,
    });
    render(
      <RequestLogTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );
    expect(screen.getByText("500")).toBeInTheDocument();
  });

  it("slides kept rows down and highlights rows that arrive on refresh", () => {
    const animate = vi.fn();
    const original = Element.prototype.animate;
    Element.prototype.animate = animate;
    const logsOf = (...ids: string[]) => ({
      data: {
        data: ids.map((requestId) => ({
          ...base,
          requestId,
          appType: "claude",
          inputTokens: 1,
          cacheReadTokens: 0,
          cacheCreationTokens: 0,
          latencyMs: 100,
          statusCode: 200,
        })),
        total: ids.length,
        page: 0,
        pageSize: 20,
      },
      isLoading: false,
    });
    try {
      useRequestLogsMock.mockReturnValue(logsOf("a", "b"));
      const { rerender } = render(
        <RequestLogTable range={{ preset: "today" }} refreshIntervalMs={0} />,
      );
      // 第一次显示不算新来的
      expect(animate).not.toHaveBeenCalled();

      useRequestLogsMock.mockReturnValue(logsOf("c", "a", "b"));
      rerender(
        <RequestLogTable range={{ preset: "today" }} refreshIntervalMs={0} />,
      );
      const rows = screen.getAllByRole("row").slice(1);
      const targets = animate.mock.contexts as Element[];
      // 新行：每格一段渐隐底色 + 整行淡入
      expect(targets).toContain(rows[0]);
      expect(targets).toContain(rows[0].querySelector("td"));
      // 旧行各往下挪一行
      const shiftOf = (row: Element) =>
        animate.mock.calls[targets.indexOf(row)]?.[0]?.[0]?.transform;
      expect(shiftOf(rows[1])).toBe("translateY(-100%)");
      expect(shiftOf(rows[2])).toBe("translateY(-100%)");
    } finally {
      Element.prototype.animate = original;
    }
  });

  it("does not animate rows that only appear because the page size grew", () => {
    const animate = vi.fn();
    const original = Element.prototype.animate;
    Element.prototype.animate = animate;
    const logsOf = (...ids: string[]) => ({
      data: {
        data: ids.map((requestId) => ({
          ...base,
          requestId,
          appType: "claude",
          inputTokens: 1,
          cacheReadTokens: 0,
          cacheCreationTokens: 0,
          latencyMs: 100,
          statusCode: 200,
        })),
        total: 40,
        page: 0,
        pageSize: ids.length,
      },
      isLoading: false,
    });
    try {
      useRequestLogsMock.mockReturnValue(logsOf("a", "b"));
      const { rerender } = render(
        <RequestLogTable
          range={{ preset: "today" }}
          refreshIntervalMs={0}
          pageSize={10}
        />,
      );
      // 每页从 10 条调到 20 条：c、d 是这一屏多出来的，不是刚进来的请求
      useRequestLogsMock.mockReturnValue(logsOf("a", "b", "c", "d"));
      rerender(
        <RequestLogTable
          range={{ preset: "today" }}
          refreshIntervalMs={0}
          pageSize={20}
        />,
      );
      expect(animate).not.toHaveBeenCalled();
    } finally {
      Element.prototype.animate = original;
    }
  });

  it("formats hit percentages without rounding up to 100", () => {
    expect(formatHitPercent(1)).toBe("100%");
    expect(formatHitPercent(0.99996)).toBe("99.9%");
    expect(formatHitPercent(0.5)).toBe("50.0%");
    expect(formatHitPercent(0)).toBe("0.0%");
  });
});

describe("formatLogTime", () => {
  const at = (
    y: number,
    m: number,
    d: number,
    h: number,
    mi: number,
    s: number,
  ) => Math.floor(new Date(y, m - 1, d, h, mi, s).getTime() / 1000);

  it("shows only the clock (with seconds) for requests from today in local time", () => {
    const now = new Date(2026, 9, 2, 18, 0, 0);
    expect(formatLogTime(at(2026, 10, 2, 14, 32, 5), now)).toBe("14:32:05");
    expect(formatLogTime(at(2026, 10, 2, 0, 0, 1), now)).toBe("00:00:01");
  });

  it("keeps the date for requests from other days", () => {
    const now = new Date(2026, 9, 2, 0, 30, 0);
    expect(formatLogTime(at(2026, 10, 1, 23, 59, 59), now)).toBe("10-01 23:59");
    // 同月同日但不同年也不算今天
    expect(formatLogTime(at(2025, 10, 2, 9, 5, 0), now)).toBe("10-02 09:05");
  });
});
