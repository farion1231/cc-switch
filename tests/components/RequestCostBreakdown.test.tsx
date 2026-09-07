import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { RequestCostBreakdown } from "@/components/usage/RequestCostBreakdown";
import type { RequestLog } from "@/types/usage";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("stored request cost breakdown", () => {
  it("shows historical component rates, cache costs and the multiplier separately", () => {
    const log: RequestLog = {
      requestId: "historical",
      providerId: "p",
      appType: "claude",
      model: "claude-opus-5",
      serviceTier: "fast",
      serviceTierSource: "response",
      reasoningEffort: "high",
      inputTokens: 1000,
      outputTokens: 100,
      cacheReadTokens: 2000,
      cacheCreationTokens: 500,
      inputCostUsd: "0.01",
      outputCostUsd: "0.005",
      cacheReadCostUsd: "0.002",
      cacheCreationCostUsd: "0.00625",
      totalCostUsd: "0.034875",
      costMultiplier: "1.5",
      isStreaming: true,
      latencyMs: 1,
      statusCode: 200,
      createdAt: 1,
    };
    render(<RequestCostBreakdown log={log} locale="en-US" />);
    expect(
      screen.getByText("1,000 × $10.000000/M = $0.010000"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("100 × $50.000000/M = $0.005000"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("2,000 × $1.000000/M = $0.002000"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("500 × $12.500000/M = $0.006250"),
    ).toBeInTheDocument();
    expect(screen.getByText("$0.023250 × 1.5 = $0.034875")).toBeInTheDocument();
    expect(screen.getByText(/Fast/)).toHaveClass("text-green-700");
  });
  it.each([
    ["0", "usage.costBreakdownUnavailable", "63 × --/M = --"],
    ["0.01", "usage.costBreakdownMismatch", "63 × $158.730159/M = $0.010000"],
  ])(
    "preserves reported totals without a false equation (%s)",
    (inputCost, notice, inputText) => {
      const log = {
        requestId: "grok-reported",
        providerId: "local",
        appType: "grokbuild",
        model: "grok-4.6-build",
        inputTokens: 63,
        outputTokens: 77,
        cacheReadTokens: 266112,
        cacheCreationTokens: 0,
        inputCostUsd: inputCost,
        outputCostUsd: "0",
        cacheReadCostUsd: "0",
        cacheCreationCostUsd: "0",
        totalCostUsd: "0.045439",
        costMultiplier: "1",
        isStreaming: false,
        latencyMs: 1,
        statusCode: 200,
        createdAt: 1,
      } satisfies RequestLog;
      render(<RequestCostBreakdown log={log} locale="en-US" />);
      expect(screen.getByText(notice)).toBeInTheDocument();
      expect(screen.getByText(inputText)).toBeInTheDocument();
      expect(screen.getByText("$0.045439")).toBeInTheDocument();
      expect(screen.queryByText(/= \$0.045439/)).not.toBeInTheDocument();
    },
  );
});
