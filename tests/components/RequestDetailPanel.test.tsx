import { fireEvent, screen } from "@testing-library/react";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { describe, expect, it, vi } from "vitest";
import { RequestDetailPanel } from "@/components/usage/RequestDetailPanel";
import type { RequestLog } from "@/types/usage";

const useRequestDetailMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options && "value" in options ? `${key}:${options.value}` : key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useRequestDetail: (id: string) => useRequestDetailMock(id),
}));

const log = (overrides: Partial<RequestLog>): RequestLog => ({
  requestId: "req_1",
  providerId: "p1",
  providerName: "GitHub Copilot",
  appType: "claude",
  model: "claude-opus-5-5",
  costMultiplier: "1",
  inputTokens: 3_066,
  outputTokens: 1_512,
  cacheReadTokens: 71_300,
  cacheCreationTokens: 2_048,
  inputCostUsd: "0.0123",
  outputCostUsd: "0.0302",
  cacheReadCostUsd: "0.0143",
  cacheCreationCostUsd: "0.0102",
  totalCostUsd: "0.0670",
  isStreaming: true,
  latencyMs: 26_400,
  firstTokenMs: 3_200,
  statusCode: 200,
  createdAt: 1_759_212_000,
  ...overrides,
});

const renderWith = (request: RequestLog) => {
  useRequestDetailMock.mockReturnValue({ data: request, isLoading: false });
  render(
    <RequestDetailPanel requestId={request.requestId} onClose={vi.fn()} />,
  );
};

describe("RequestDetailPanel", () => {
  it("marks every cost component unknown when pricing is missing", () => {
    renderWith(log({ costKnown: false, totalCostUsd: "0" }));
    expect(screen.getAllByText("usage.unpriced").length).toBeGreaterThanOrEqual(
      5,
    );
    expect(screen.queryByText("$0.0000")).not.toBeInTheDocument();
  });

  it("shows a genuine zero price as free", () => {
    renderWith(
      log({
        costKnown: true,
        totalCostUsd: "0",
        inputCostUsd: "0",
        outputCostUsd: "0",
        cacheReadCostUsd: "0",
        cacheCreationCostUsd: "0",
      }),
    );
    expect(screen.queryByText("usage.unpriced")).not.toBeInTheDocument();
    expect(screen.getAllByText("$0.0000").length).toBeGreaterThanOrEqual(5);
  });

  it("shows speed, first token and duration for routed requests", () => {
    renderWith(log({}));

    expect(screen.getByText("usage.requestDetail")).toBeInTheDocument();
    // 1512 / (26.4 - 3.2) ≈ 65
    expect(screen.getByText("usage.speedValue:65")).toBeInTheDocument();
    expect(screen.getByText("3.2s")).toBeInTheDocument();
    expect(screen.getByText("26.4s")).toBeInTheDocument();
    expect(screen.getByText("$0.0670")).toBeInTheDocument();
  });

  it("explains why short outputs have no speed", () => {
    renderWith(
      log({ outputTokens: 64, latencyMs: 2_400, firstTokenMs: 1_900 }),
    );

    expect(screen.getByText("usage.detail.speedTooFew")).toBeInTheDocument();
  });

  it("shows an estimated speed and duration for session logs", () => {
    renderWith(
      log({
        dataSource: "session_log",
        firstTokenMs: undefined,
        outputTokens: 1_800,
        latencyMs: 20_000,
      }),
    );

    // 1800 / 20 s，含首字等待
    expect(
      screen.getByText("usage.speedEstimatedValue:90"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("usage.detail.durationEstimated"),
    ).toBeInTheDocument();
    expect(screen.getByText("20.0s")).toBeInTheDocument();
    expect(screen.queryByText("usage.detail.firstToken")).toBeNull();
  });

  it.each(["codex_session", "kimi_session"])(
    "shows the estimated first token for %s",
    (dataSource) => {
      renderWith(
        log({
          dataSource,
          firstTokenMs: 3_300,
          outputTokens: 1_800,
          latencyMs: 20_000,
        }),
      );

      expect(
        screen.getByText("usage.detail.firstTokenEstimated"),
      ).toBeInTheDocument();
      expect(screen.getByText("3.3s")).toBeInTheDocument();
      // 速度仍按整段耗时估
      expect(
        screen.getByText("usage.speedEstimatedValue:90"),
      ).toBeInTheDocument();
      expect(screen.queryByText("usage.detail.firstToken")).toBeNull();
    },
  );

  it("shows Kimi's actual model and first token while keeping the short-output speed threshold", () => {
    renderWith(
      log({
        appType: "kimi",
        providerId: "_kimi_session",
        providerName: "Kimi Code (Session)",
        dataSource: "kimi_session",
        model: "agnes-3.0-flash",
        firstTokenMs: 10_061,
        latencyMs: 10_228,
        outputTokens: 2,
      }),
    );
    expect(screen.getByText("agnes-3.0-flash")).toBeInTheDocument();
    expect(
      screen.getByText("usage.detail.firstTokenEstimated"),
    ).toBeInTheDocument();
    expect(screen.getByText("10.1s")).toBeInTheDocument();
    expect(screen.getByText("10.2s")).toBeInTheDocument();
    expect(
      screen.getByText("usage.detail.speedEstimateTooFew"),
    ).toBeInTheDocument();
  });

  it("explains why short session-log outputs have no estimate", () => {
    renderWith(
      log({
        dataSource: "session_log",
        firstTokenMs: undefined,
        outputTokens: 120,
        latencyMs: 6_000,
      }),
    );

    expect(
      screen.getByText("usage.detail.speedEstimateTooFew"),
    ).toBeInTheDocument();
    expect(screen.getByText("6.0s")).toBeInTheDocument();
  });

  it("says session logs have no timing data", () => {
    renderWith(
      log({
        dataSource: "codex_session",
        firstTokenMs: undefined,
        latencyMs: 0,
      }),
    );

    expect(
      screen.getByText("usage.detail.noTimingSession"),
    ).toBeInTheDocument();
    // 供应商已经叫「会话日志」：不再重复列来源和内部的供应商 ID
    expect(screen.queryByText("usage.detail.source")).not.toBeInTheDocument();
    expect(
      screen.queryByText("usage.detail.providerId"),
    ).not.toBeInTheDocument();
  });

  it("shows the error and a retry button when loading fails", () => {
    const refetch = vi.fn();
    useRequestDetailMock.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: "ambiguous column name: created_at",
      refetch,
    });
    render(<RequestDetailPanel requestId="req_1" onClose={vi.fn()} />);

    expect(screen.getByText("usage.requestLoadFailed")).toBeInTheDocument();
    expect(
      screen.getByText("ambiguous column name: created_at"),
    ).toBeInTheDocument();
    expect(screen.queryByText("usage.requestNotFound")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
    expect(refetch).toHaveBeenCalledTimes(1);
  });

  it("says the request is not found only when the query returned nothing", () => {
    useRequestDetailMock.mockReturnValue({
      data: null,
      isLoading: false,
      error: null,
    });
    render(<RequestDetailPanel requestId="req_1" onClose={vi.fn()} />);

    expect(screen.getByText("usage.requestNotFound")).toBeInTheDocument();
    expect(screen.queryByText("usage.requestLoadFailed")).toBeNull();
  });
});
