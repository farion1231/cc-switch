import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { RequestDetailPanel } from "@/components/usage/RequestDetailPanel";
import type { RequestLog } from "@/types/usage";

const { mockUseRequestDetail } = vi.hoisted(() => ({
  mockUseRequestDetail: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, defaultValueOrOpts?: unknown) => {
      if (
        defaultValueOrOpts &&
        typeof defaultValueOrOpts === "object" &&
        defaultValueOrOpts !== null &&
        "defaultValue" in (defaultValueOrOpts as object)
      ) {
        const opts = defaultValueOrOpts as {
          defaultValue?: string;
          count?: number;
        };
        let s = opts.defaultValue ?? key;
        if (opts.count != null) {
          s = s.replace("{{count}}", String(opts.count));
        }
        return s;
      }
      if (typeof defaultValueOrOpts === "string") return defaultValueOrOpts;
      return key;
    },
    i18n: { language: "zh", resolvedLanguage: "zh" },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useRequestDetail: (requestId: string) => mockUseRequestDetail(requestId),
}));

function baseLog(over: Partial<RequestLog> = {}): RequestLog {
  return {
    requestId: "req-1",
    providerId: "p1",
    appType: "codex",
    model: "gpt-5",
    costMultiplier: "1",
    inputTokens: 10,
    outputTokens: 20,
    cacheReadTokens: 0,
    cacheCreationTokens: 0,
    inputCostUsd: "0",
    outputCostUsd: "0",
    cacheReadCostUsd: "0",
    cacheCreationCostUsd: "0",
    totalCostUsd: "0.01",
    isStreaming: false,
    latencyMs: 100,
    statusCode: 200,
    createdAt: 1_710_000_000,
    reasoningTokens: 500,
    reasoningSource: "proxy_response",
    continuationStatus: "continued",
    continuationRounds: 2,
    sessionEnriched: true,
    turnId: "turn-1",
    promptReplaced: true,
    identityCorrected: false,
    promptFingerprint: "fp-abc",
    ...over,
  };
}

describe("RequestDetailPanel reasoning contract", () => {
  beforeEach(() => {
    mockUseRequestDetail.mockReset();
  });

  it("shows continuation metadata without reasoning body", () => {
    mockUseRequestDetail.mockReturnValue({
      data: baseLog(),
      isLoading: false,
    });

    render(
      <RequestDetailPanel requestId="req-1" onClose={() => undefined} />,
    );

    expect(screen.getByText("proxy_response")).toBeInTheDocument();
    expect(screen.getByText("2 个续接轮次")).toBeInTheDocument();
    expect(screen.queryByText(/encrypted_content/)).not.toBeInTheDocument();
    expect(screen.queryByText(/reasoning body/i)).not.toBeInTheDocument();
    expect(screen.getByTestId("session-enriched")).toBeInTheDocument();
    expect(screen.getByText(/fp-abc/)).toBeInTheDocument();
    expect(screen.queryByText(/You are ChatGPT/i)).not.toBeInTheDocument();
  });

  it("renders dash-style empty reasoning when tokens null", () => {
    mockUseRequestDetail.mockReturnValue({
      data: baseLog({
        reasoningTokens: null,
        continuationRounds: 0,
        continuationStatus: "not_attempted",
        sessionEnriched: false,
        promptReplaced: false,
        promptFingerprint: undefined,
        reasoningSource: undefined,
      }),
      isLoading: false,
    });

    render(
      <RequestDetailPanel requestId="req-empty" onClose={() => undefined} />,
    );
    expect(screen.getByText("—")).toBeInTheDocument();
  });
});
