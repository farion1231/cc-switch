import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UsageHeatmap } from "@/components/usage/UsageHeatmap";
import type { DailyStats } from "@/types/usage";

const trends = vi.hoisted(() => vi.fn());

vi.mock("@/lib/query/usage", () => ({
  useUsageFirstDate: () => ({ data: "2025-06-01", isSuccess: true }),
  useUsageTrends: (...args: unknown[]) => trends(...args),
}));

vi.mock("@/hooks/useMoneyFormat", () => ({
  useMoneyFormat: () => ({ money: (value: number) => `$${value.toFixed(2)}` }),
}));

function day(
  dayOfMonth: number,
  appTypes?: string[],
  requestCount = 1,
): DailyStats {
  return {
    date: new Date(2025, 5, dayOfMonth).toISOString(),
    appTypes,
    requestCount,
    totalTokens: 10,
    totalCost: "0",
    totalInputTokens: 10,
    totalOutputTokens: 0,
    totalCacheCreationTokens: 0,
    totalCacheReadTokens: 0,
  };
}

function fills(cell: HTMLElement) {
  return Array.from(cell.children, (child) =>
    (child as HTMLElement).style.getPropertyValue("--heat-fill"),
  );
}

describe("UsageHeatmap runtime colors", () => {
  beforeEach(() => {
    vi.setSystemTime(new Date(2025, 5, 8, 12));
    trends.mockReset();
  });

  afterEach(() => vi.useRealTimers());

  it("splits mixed-runtime days and preserves the request intensity scale", () => {
    trends.mockReturnValue({
      data: [
        day(1, ["claude"]),
        day(2, ["codex"]),
        day(3, ["claude", "codex"], 10),
        day(4, ["dsh"]),
        day(5, undefined),
        day(6, ["gemini"], 0),
      ],
    });
    const { container } = render(
      <UsageHeatmap metric="requests" refreshIntervalMs={0} />,
    );
    expect(fills(screen.getByRole("img", { name: /Claude Code$/ }))).toEqual([
      expect.stringContaining("var(--agent-claude) var(--heat-strength-1)"),
    ]);
    const mixed = screen.getByRole("img", { name: /Claude Code.*Codex/ });
    expect(fills(mixed)).toEqual([
      expect.stringContaining("var(--agent-claude) var(--heat-strength-4)"),
      expect.stringContaining("var(--agent-codex) var(--heat-strength-4)"),
    ]);
    expect(screen.getByText("DeepSeek Harness")).toBeInTheDocument();
    const scale = Array.from(
      container.querySelectorAll<HTMLElement>('[style*="--text-2"]'),
    );
    expect(
      scale.map((swatch) => swatch.style.getPropertyValue("--heat-fill")),
    ).toEqual(
      [1, 2, 3, 4].map(
        (level) =>
          `color-mix(in srgb, var(--text-2) var(--heat-strength-${level}), transparent)`,
      ),
    );
    expect(screen.queryByText("Gemini CLI")).not.toBeInTheDocument();
    expect(
      container.querySelector('.usage-heat-fill.flex-1[style*="--heat-1"]'),
    ).toBeInTheDocument();
    fireEvent.mouseEnter(mixed);
    expect(
      within(screen.getByRole("tooltip")).getByText("Claude Code · Codex"),
    ).toBeInTheDocument();
  });

  it("keeps the selected runtime, provider and model filters on the trend query", () => {
    trends.mockReturnValue({ data: [day(2, ["codex"])] });
    render(
      <UsageHeatmap
        metric="hitRate"
        appType="codex"
        providerName="Example"
        model="model-a"
        refreshIntervalMs={0}
      />,
    );
    expect(trends).toHaveBeenCalledWith(
      expect.anything(),
      {
        appType: "codex",
        providerName: "Example",
        model: "model-a",
      },
      expect.anything(),
    );
    expect(fills(screen.getByRole("img", { name: /Codex$/ }))).toEqual([
      expect.stringContaining("var(--agent-codex) var(--heat-strength-1)"),
    ]);
    expect(screen.getAllByRole("img")).toHaveLength(1);
  });
});
