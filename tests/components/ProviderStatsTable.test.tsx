import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  ProviderStatsTable,
  getProviderEstimatedSpeed,
  getProviderSpeed,
} from "@/components/usage/ProviderStatsTable";
import type { ProviderStats } from "@/types/usage";

const useProviderStatsMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useProviderStats: (...args: unknown[]) => useProviderStatsMock(...args),
}));

const stat = (overrides: Partial<ProviderStats>): ProviderStats => ({
  providerId: "p",
  providerName: "P",
  requestCount: 1,
  totalTokens: 1_000,
  totalCost: "1",
  successRate: 100,
  avgLatencyMs: 1_000,
  ...overrides,
});

describe("ProviderStatsTable", () => {
  it("computes speed as total output over total generation time", () => {
    expect(
      getProviderSpeed(
        stat({ speedOutputTokens: 1_200, speedGenerationMs: 10_500 }),
      ),
    ).toBe("114");
    expect(
      getProviderSpeed(stat({ speedOutputTokens: 0, speedGenerationMs: 0 })),
    ).toBeNull();
    // 老后端没有这两个字段
    expect(getProviderSpeed(stat({}))).toBeNull();
  });

  it("falls back to the estimated speed, marked with ≈, when there is no exact one", () => {
    expect(
      getProviderEstimatedSpeed(
        stat({ estSpeedOutputTokens: 2_500, estSpeedDurationMs: 25_000 }),
      ),
    ).toBe("100");
    expect(getProviderEstimatedSpeed(stat({}))).toBeNull();

    useProviderStatsMock.mockReturnValue({
      isLoading: false,
      data: [
        stat({
          providerId: "_session",
          providerName: "Claude session",
          requestCount: 9,
          estSpeedOutputTokens: 2_500,
          estSpeedDurationMs: 25_000,
        }),
        // 两种都有时用精确的，不带 ≈
        stat({
          providerId: "mixed",
          providerName: "Mixed",
          requestCount: 3,
          speedOutputTokens: 9_200,
          speedGenerationMs: 100_000,
          estSpeedOutputTokens: 2_500,
          estSpeedDurationMs: 25_000,
        }),
      ],
    });

    render(
      <ProviderStatsTable range={{ preset: "7d" }} refreshIntervalMs={0} />,
    );

    const rows = screen.getAllByRole("row").slice(1);
    expect(rows[0].lastElementChild).toHaveTextContent("≈100tok/s");
    expect(rows[1].lastElementChild).toHaveTextContent("92tok/s");
    expect(rows[1].lastElementChild).not.toHaveTextContent("≈");
  });

  it("sorts by requests and replaces average latency with speed", () => {
    useProviderStatsMock.mockReturnValue({
      isLoading: false,
      data: [
        stat({
          providerId: "a",
          providerName: "Kimi For Coding",
          requestCount: 3,
        }),
        stat({
          providerId: "b",
          providerName: "DeepSeek",
          requestCount: 9,
          speedOutputTokens: 9_200,
          speedGenerationMs: 100_000,
        }),
      ],
    });

    render(
      <ProviderStatsTable range={{ preset: "7d" }} refreshIntervalMs={0} />,
    );

    expect(screen.queryByText("usage.avgLatency")).not.toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: /usage.speed/ }),
    ).toBeInTheDocument();
    const rows = screen.getAllByRole("row").slice(1);
    expect(rows[0]).toHaveTextContent("DeepSeek");
    expect(rows[0].lastElementChild).toHaveTextContent("92tok/s");
    expect(rows[1]).toHaveTextContent("Kimi For Coding");
    expect(rows[1].lastElementChild).toHaveTextContent("—");
  });
});
