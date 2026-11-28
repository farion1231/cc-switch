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
  apps: [appType: string, requests: number, cost?: number][] = [],
  unclassifiedRequests = 0,
): DailyStats {
  const runtimeStats = apps.map(([appType, requests, cost = 0]) => ({
    appType,
    requestCount: requests,
    totalCost: String(cost),
    totalInputTokens: requests * 10,
    totalOutputTokens: 0,
    totalCacheCreationTokens: 0,
    totalCacheReadTokens: 0,
  }));
  const requestCount = apps.reduce(
    (sum, [, requests]) => sum + requests,
    unclassifiedRequests,
  );
  return {
    date: new Date(2025, 5, dayOfMonth).toISOString(),
    runtimeStats,
    requestCount,
    totalTokens: requestCount * 10,
    totalCost: String(apps.reduce((sum, [, , cost = 0]) => sum + cost, 0)),
    totalInputTokens: requestCount * 10,
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

  it("uses one app color per day and scales intensity by the daily total", () => {
    trends.mockReturnValue({
      data: [
        day(1, [["claude", 1]]),
        day(2, [["codex", 1]]),
        day(3, [
          ["claude", 8],
          ["codex", 2],
        ]),
        day(4, [["dsh", 1]]),
        day(5, [], 1),
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
    const [claude, codex] = within(screen.getByRole("tooltip")).getAllByRole(
      "listitem",
    );
    expect(claude).toHaveTextContent(/Claude Code\s*8/);
    expect(codex).toHaveTextContent(/Codex\s*2/);
  });

  it("changes the dominant app with the selected metric and shows the cost breakdown", () => {
    trends.mockReturnValue({
      data: [
        day(3, [
          ["claude", 8, 2],
          ["codex", 2, 8],
          ["gemini", 1],
        ]),
      ],
    });
    render(<UsageHeatmap metric="cost" refreshIntervalMs={0} />);
    const mixed = screen.getByRole("img", { name: /Claude Code.*Codex/ });
    expect(fills(mixed)).toEqual([
      expect.stringContaining("var(--agent-codex) var(--heat-strength-1)"),
    ]);
    expect(screen.getByText("Gemini CLI")).toBeInTheDocument();
    fireEvent.mouseEnter(mixed);
    const rows = within(screen.getByRole("tooltip")).getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent(/Claude Code\s*\$2\.00/);
    expect(rows[1]).toHaveTextContent(/Codex\s*\$8\.00/);
    expect(rows[2]).toHaveTextContent(/Gemini CLI\s*\$0\.00/);
  });

  it("switches between the dominant app, dominant model and daily total", () => {
    trends.mockReturnValue({
      data: [
        {
          ...day(3, [
            ["claude", 8],
            ["codex", 2],
          ]),
          modelStats: [
            {
              model: "claude-sonnet-4-6",
              requestCount: 8,
              totalCost: 0,
              totalTokens: 80,
            },
            {
              model: "gpt-5.4",
              requestCount: 2,
              totalCost: 0,
              totalTokens: 20,
            },
          ],
        },
      ],
    });
    render(<UsageHeatmap metric="requests" refreshIntervalMs={0} />);
    expect(trends).toHaveBeenLastCalledWith(
      expect.anything(),
      expect.anything(),
      expect.objectContaining({ includeModels: false }),
    );

    fireEvent.click(
      screen.getByRole("button", { name: "usage.trend.byModel" }),
    );
    expect(trends).toHaveBeenLastCalledWith(
      expect.anything(),
      expect.anything(),
      expect.objectContaining({ includeModels: true }),
    );
    const modelCell = screen.getByRole("img", {
      name: /claude-sonnet-4-6.*gpt-5.4/,
    });
    const modelSwatch = screen.getByText("claude-sonnet-4-6")
      .previousElementSibling as HTMLElement;
    expect(fills(modelCell)).toEqual([
      modelSwatch.style
        .getPropertyValue("--heat-fill")
        .replace("--heat-strength-4", "--heat-strength-1"),
    ]);
    fireEvent.mouseEnter(modelCell);
    expect(
      within(screen.getByRole("tooltip")).getByText("claude-sonnet-4-6"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "common.all" }));
    expect(fills(screen.getByRole("img"))).toEqual(["var(--heat-1)"]);
    expect(screen.queryByText("claude-sonnet-4-6")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "usage.trend.byApp" }));
    expect(
      fills(screen.getByRole("img", { name: /Claude Code.*Codex/ })),
    ).toEqual([expect.stringContaining("var(--agent-claude)")]);
  });

  it("keeps the selected runtime, provider and model filters on the trend query", () => {
    trends.mockReturnValue({ data: [day(2, [["codex", 1]])] });
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
