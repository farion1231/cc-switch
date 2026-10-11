import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderStatsTable } from "@/components/usage/ProviderStatsTable";
import {
  getStatsEstimatedSpeed,
  getStatsSpeed,
} from "@/components/usage/statsColumns";
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
  appType: "claude",
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
      getStatsSpeed(
        stat({ speedOutputTokens: 1_200, speedGenerationMs: 10_500 }),
      ),
    ).toBe("114");
    expect(
      getStatsSpeed(stat({ speedOutputTokens: 0, speedGenerationMs: 0 })),
    ).toBeNull();
    // 老后端没有这两个字段
    expect(getStatsSpeed(stat({}))).toBeNull();
  });

  it("falls back to the estimated speed, marked with ≈, when there is no exact one", () => {
    expect(
      getStatsEstimatedSpeed(
        stat({ estSpeedOutputTokens: 2_500, estSpeedDurationMs: 25_000 }),
      ),
    ).toBe("100");
    expect(getStatsEstimatedSpeed(stat({}))).toBeNull();

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

  it.each([false, true])(
    "removes cross-app rows after repeated switches (empty target: %s)",
    (emptyTarget) => {
      const consoleError = vi.spyOn(console, "error");
      const claudeStats = [
        stat({
          providerId: "shared-provider",
          appType: "claude-desktop",
          providerName: "Shared Provider",
          requestCount: 42,
        }),
        stat({
          providerId: "shared-provider",
          appType: "claude",
          providerName: "Shared Provider",
          requestCount: 7,
        }),
        stat({ providerId: "other", providerName: "Other" }),
      ];
      const targetStats = emptyTarget
        ? []
        : [
            stat({
              providerId: "codex-only",
              appType: "codex",
              providerName: "Codex only",
              requestCount: 10,
            }),
            stat({
              providerId: "other",
              appType: "codex",
              providerName: "Other",
            }),
          ];
      const table = (appType: string) => (
        <ProviderStatsTable
          range={{ preset: "7d" }}
          appType={appType}
          refreshIntervalMs={0}
        />
      );

      try {
        useProviderStatsMock.mockReturnValue({
          isLoading: false,
          data: claudeStats,
        });
        const { rerender } = render(table("claude"));

        for (let cycle = 0; cycle < 2; cycle++) {
          if (cycle > 0) {
            useProviderStatsMock.mockReturnValue({
              isLoading: false,
              data: claudeStats,
            });
            rerender(table("claude"));
          }
          const claudeRows = screen.getAllByRole("row").slice(1);
          expect(claudeRows).toHaveLength(3);
          expect(claudeRows[0]).toHaveTextContent("Shared Provider");
          expect(claudeRows[0]).toHaveTextContent("42");
          expect(claudeRows[1]).toHaveTextContent("Shared Provider");
          expect(claudeRows[1]).toHaveTextContent("7");

          useProviderStatsMock.mockReturnValue({
            isLoading: false,
            data: targetStats,
          });
          rerender(table("codex"));
          expect(screen.queryByText("Shared Provider")).not.toBeInTheDocument();
          const targetRows = screen.getAllByRole("row").slice(1);
          expect(targetRows).toHaveLength(emptyTarget ? 1 : 2);
          if (emptyTarget) {
            expect(targetRows[0]).toHaveTextContent("usage.noData");
          } else {
            expect(targetRows[0]).toHaveTextContent("Codex only");
            expect(targetRows[1]).toHaveTextContent("Other");
          }
        }

        useProviderStatsMock.mockReturnValue({ isLoading: false, data: [] });
        rerender(table("gemini"));
        expect(screen.queryByText("Shared Provider")).not.toBeInTheDocument();
        expect(screen.getAllByRole("row")).toHaveLength(2);
        expect(screen.getByText("usage.noData")).toBeInTheDocument();
        expect(
          consoleError.mock.calls.filter((args) =>
            args.some((arg) => String(arg).includes("same key")),
          ),
        ).toHaveLength(0);
      } finally {
        consoleError.mockRestore();
      }
    },
  );
});
