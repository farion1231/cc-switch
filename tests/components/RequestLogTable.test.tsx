import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  RequestLogTable,
  appShortName,
  formatLogTime,
} from "@/components/usage/RequestLogTable";
import type { RequestLog, UsageRangeSelection } from "@/types/usage";
import { REQUEST_LOG_COLUMNS } from "@/components/usage/tableColumns";

const useRequestLogsMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (
      key: string,
      options?: {
        defaultValue?: string;
        value?: string;
      },
    ) =>
      key === "usage.firstTokenMilliseconds"
        ? `${key}:${options?.value}`
        : (options?.defaultValue ?? key),
    i18n: {
      resolvedLanguage: "en",
      language: "en",
    },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useRequestLogs: (args: unknown) => useRequestLogsMock(args),
}));

const requestLog = (overrides: Partial<RequestLog> = {}): RequestLog => ({
  requestId: "r1",
  providerId: "p1",
  providerName: "DeepSeek",
  appType: "codex",
  model: "deepseek-v4-pro",
  costMultiplier: "1",
  inputTokens: 1_000,
  outputTokens: 1_100,
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
  latencyMs: 12_900,
  firstTokenMs: 1_800,
  ...overrides,
});

const showLogs = (logs: RequestLog[]) => {
  useRequestLogsMock.mockReturnValue({
    data: { data: logs, total: logs.length, page: 0, pageSize: 20 },
    isLoading: false,
  });
};

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

  it("shows ten columns by default with first token immediately before speed", () => {
    showLogs([requestLog()]);
    render(
      <RequestLogTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );

    const headers = screen.getAllByRole("columnheader");
    expect(headers).toHaveLength(10);
    expect(headers[8]).toHaveTextContent("usage.firstToken");
    expect(headers[9]).toHaveTextContent("usage.speed");
    const cells = within(screen.getAllByRole("row")[1]).getAllByRole("cell");
    expect(cells).toHaveLength(10);
    expect(cells[8]).toHaveTextContent("1.8s");
    expect(cells[8]).toHaveAttribute(
      "title",
      "usage.firstTokenMilliseconds:1,800",
    );
  });

  it.each([
    [0, "0.0s", "usage.firstTokenMilliseconds:0"],
    [1_800.5, "1.8s", "usage.firstTokenMilliseconds:1,800.5"],
    [undefined, "—", null],
    [null, "—", null],
    [-1, "—", null],
    [Number.NaN, "—", null],
    [Number.POSITIVE_INFINITY, "—", null],
  ])(
    "renders firstTokenMs=%s without inventing timing",
    (value, text, title) => {
      showLogs([requestLog({ firstTokenMs: value as number | undefined })]);
      render(
        <RequestLogTable range={{ preset: "today" }} refreshIntervalMs={0} />,
      );

      const cell = within(screen.getAllByRole("row")[1]).getAllByRole(
        "cell",
      )[8];
      expect(cell).toHaveTextContent(text);
      expect(cell.getAttribute("title")).toBe(title);
    },
  );

  it("keeps headers, data cells, empty state and width aligned with visibility", () => {
    showLogs([requestLog()]);
    const visibility = { provider: false, model: false, firstToken: false };
    const { rerender } = render(
      <RequestLogTable
        range={{ preset: "today" }}
        refreshIntervalMs={0}
        columnVisibility={visibility}
      />,
    );

    expect(screen.getAllByRole("columnheader")).toHaveLength(7);
    expect(
      within(screen.getAllByRole("row")[1]).getAllByRole("cell"),
    ).toHaveLength(7);
    expect(
      screen.queryByRole("columnheader", { name: "usage.firstToken" }),
    ).toBeNull();
    expect(screen.getByRole("table")).toHaveStyle({ minWidth: "404px" });

    showLogs([]);
    rerender(
      <RequestLogTable
        range={{ preset: "today" }}
        refreshIntervalMs={0}
        columnVisibility={visibility}
      />,
    );
    expect(screen.getByRole("cell", { name: "usage.noData" })).toHaveAttribute(
      "colspan",
      "7",
    );
  });

  it("preserves the time and detail entry when every column is requested hidden", () => {
    showLogs([requestLog()]);
    const onOpenDetail = vi.fn();
    render(
      <RequestLogTable
        range={{ preset: "today" }}
        refreshIntervalMs={0}
        columnVisibility={Object.fromEntries(
          REQUEST_LOG_COLUMNS.map(({ id }) => [id, false]),
        )}
        onOpenDetail={onOpenDetail}
      />,
    );

    expect(screen.getAllByRole("columnheader")).toHaveLength(1);
    expect(screen.getByRole("columnheader")).toHaveTextContent("usage.time");
    const row = screen.getAllByRole("row")[1];
    expect(within(row).getAllByRole("cell")).toHaveLength(1);
    fireEvent.click(
      within(row).getByRole("button", { name: "usage.openRequestDetail" }),
    );
    expect(onOpenDetail).toHaveBeenCalledTimes(1);
    expect(onOpenDetail).toHaveBeenCalledWith("r1");
    fireEvent.click(row);
    expect(onOpenDetail).toHaveBeenCalledTimes(2);
  });

  it("does not reset pagination when only column visibility changes", async () => {
    const range: UsageRangeSelection = { preset: "today" };
    const { rerender } = render(
      <RequestLogTable range={range} refreshIntervalMs={0} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "usage.nextPage" }));
    await waitFor(() =>
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({ page: 1 }),
      ),
    );

    rerender(
      <RequestLogTable
        range={range}
        refreshIntervalMs={0}
        columnVisibility={{ firstToken: false }}
      />,
    );
    expect(useRequestLogsMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ page: 1 }),
    );
    expect(screen.getAllByRole("columnheader")).toHaveLength(9);
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
        ],
        total: 4,
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
    expect(rows).toHaveLength(4);
    expect(rows[0].lastElementChild).toHaveTextContent("99tok/s");
    expect(rows[0].lastElementChild).toHaveAttribute(
      "title",
      "usage.timingTip",
    );
    expect(rows[1].lastElementChild).toHaveTextContent("—");
    expect(within(rows[1]).getAllByRole("cell")[8]).toHaveTextContent("1.9s");
    expect(rows[2].lastElementChild).toHaveTextContent("—");
    expect(within(rows[2]).getAllByRole("cell")[8]).toHaveTextContent("—");
    expect(rows[3].lastElementChild).toHaveTextContent("≈90tok/s");
    expect(within(rows[3]).getAllByRole("cell")[8]).toHaveTextContent("—");
    expect(rows[3].lastElementChild).toHaveAttribute(
      "title",
      "usage.estimatedTimingTip",
    );
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
