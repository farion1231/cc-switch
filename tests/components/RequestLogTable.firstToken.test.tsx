import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { RequestLogTable } from "@/components/usage/RequestLogTable";

const useRequestLogsMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key,
    i18n: {
      resolvedLanguage: "en",
      language: "en",
    },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useRequestLogs: (args: unknown) => useRequestLogsMock(args),
}));

describe("RequestLogTable first token column", () => {
  it("renders first token column with formatted value", () => {
    useRequestLogsMock.mockReturnValue({
      data: {
        data: [
          {
            requestId: "1",
            providerId: "p1",
            providerName: "Test",
            appType: "claude",
            model: "test-model",
            costMultiplier: "1",
            inputTokens: 100,
            outputTokens: 50,
            cacheReadTokens: 0,
            cacheCreationTokens: 0,
            inputCostUsd: "0",
            outputCostUsd: "0",
            cacheReadCostUsd: "0",
            cacheCreationCostUsd: "0",
            totalCostUsd: "0",
            isStreaming: true,
            statusCode: 200,
            latencyMs: 5000,
            firstTokenMs: 1500,
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

    expect(screen.getByText("1.5s")).toBeInTheDocument();
  });

  it("renders dash when firstTokenMs is null", () => {
    useRequestLogsMock.mockReturnValue({
      data: {
        data: [
          {
            requestId: "1",
            providerId: "p1",
            providerName: "Test",
            appType: "claude",
            model: "test-model",
            costMultiplier: "1",
            inputTokens: 100,
            outputTokens: 50,
            cacheReadTokens: 0,
            cacheCreationTokens: 0,
            inputCostUsd: "0",
            outputCostUsd: "0",
            cacheReadCostUsd: "0",
            cacheCreationCostUsd: "0",
            totalCostUsd: "0",
            isStreaming: true,
            statusCode: 200,
            latencyMs: 3000,
            firstTokenMs: null,
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

    const dashes = screen.getAllByText("—");
    expect(dashes.length).toBeGreaterThan(0);
  });

  it("renders the first token column header", () => {
    useRequestLogsMock.mockReturnValue({
      data: {
        data: [],
        total: 0,
        page: 0,
        pageSize: 20,
      },
      isLoading: false,
    });

    render(
      <RequestLogTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );

    expect(
      screen.getByRole("columnheader", { name: /usage.firstToken/ }),
    ).toBeInTheDocument();
  });
});
