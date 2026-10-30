import { act, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { ProjectUsageBreakdown } from "@/components/usage/ProjectUsageBreakdown";
import type { ProviderStats, UsageRangeSelection } from "@/types/usage";
import type { UsageShareDatum } from "@/components/usage/UsageShareChart";

const useModelStatsMock = vi.hoisted(() => vi.fn());
const useProviderStatsMock = vi.hoisted(() => vi.fn());
const shareCharts = vi.hoisted(
  () =>
    [] as Array<{
      onSelect?: (key: string) => void;
      data: UsageShareDatum[];
      includeZeroValues?: boolean;
      maxSlices?: number;
    }>,
);

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: { value?: string; rate?: string }) =>
      key === "usage.share.requestCacheHit"
        ? `${values?.value} requests · Cache hit ${values?.rate}`
        : key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useModelStats: (...args: unknown[]) => useModelStatsMock(...args),
  useProviderStats: (...args: unknown[]) => useProviderStatsMock(...args),
}));

vi.mock("@/components/usage/UsageShareChart", () => ({
  UsageShareChart: (props: {
    onSelect?: (key: string) => void;
    data: UsageShareDatum[];
  }) => {
    shareCharts.push(props);
    return <div data-testid="share-chart" />;
  },
}));

const range: UsageRangeSelection = { preset: "today" };

const providerStat: ProviderStats = {
  providerId: "p-1",
  appType: "codex",
  providerName: "DeepSeek",
  requestCount: 10,
  totalTokens: 1_000,
  totalCost: "2",
  successRate: 100,
  avgLatencyMs: 0,
  freshInputTokens: 10,
  cacheCreationTokens: 0,
  cacheReadTokens: 90,
};

describe("ProjectUsageBreakdown", () => {
  beforeEach(() => {
    shareCharts.length = 0;
    useModelStatsMock.mockReset().mockReturnValue({ data: [] });
    useProviderStatsMock.mockReset().mockReturnValue({ data: [providerStat] });
  });

  it("passes provider id and app type when a provider is selected", () => {
    render(
      <table>
        <tbody>
          <ProjectUsageBreakdown
            projectKey="/repo"
            range={range}
            appType={undefined}
            refreshIntervalMs={0}
            colSpan={8}
          />
        </tbody>
      </table>,
    );

    // The first chart is the provider share list; selecting a slice narrows models.
    act(() => shareCharts[0]?.onSelect?.("codex:p-1"));

    expect(useModelStatsMock).toHaveBeenLastCalledWith(
      range,
      expect.objectContaining({
        providerId: "p-1",
        providerAppType: "codex",
      }),
      expect.anything(),
    );
  });

  it("shows model hit rates beside request counts in the share view", () => {
    useModelStatsMock.mockReturnValue({
      data: [{ ...providerStat, model: "model-a" }],
    });
    render(
      <table>
        <tbody>
          <ProjectUsageBreakdown
            projectKey="/repo"
            range={range}
            refreshIntervalMs={0}
            colSpan={8}
          />
        </tbody>
      </table>,
    );
    expect(shareCharts.at(-1)?.data[0].detail).toBe(
      "10 requests · Cache hit 90.0%",
    );
  });

  it("keeps provider identity across cache and token views and clears it on reset", async () => {
    useProviderStatsMock.mockReturnValue({
      data: [providerStat, { ...providerStat, appType: "claude" }],
    });
    useModelStatsMock.mockReturnValue({
      data: [{ ...providerStat, model: "model-a" }],
    });
    render(
      <table>
        <tbody>
          <ProjectUsageBreakdown
            projectKey="/repo"
            range={range}
            refreshIntervalMs={0}
            colSpan={8}
          />
        </tbody>
      </table>,
    );
    act(() => shareCharts[0].onSelect?.("claude:p-1"));
    await userEvent.click(
      screen.getByRole("button", { name: "usage.share.metric.cache" }),
    );
    expect(shareCharts.at(-1)?.data).toEqual([
      {
        key: "model-a",
        label: "model-a",
        value: 100,
        detail: "10 requests · Cache hit 90.0%",
      },
    ]);
    expect(useModelStatsMock).toHaveBeenLastCalledWith(
      range,
      expect.objectContaining({
        providerId: "p-1",
        providerAppType: "claude",
        projectKey: "/repo",
      }),
      expect.anything(),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "usage.share.metric.tokens" }),
    );
    expect(useModelStatsMock).toHaveBeenLastCalledWith(
      range,
      expect.objectContaining({ providerId: "p-1", providerAppType: "claude" }),
      expect.anything(),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "usage.share.allProviders" }),
    );
    expect(useModelStatsMock).toHaveBeenLastCalledWith(
      range,
      expect.objectContaining({
        providerId: undefined,
        providerAppType: undefined,
      }),
      expect.anything(),
    );
  });

  it("clears the provider selection when project or time range changes", () => {
    const component = (projectKey: string, selectedRange = range) => (
      <table>
        <tbody>
          <ProjectUsageBreakdown
            projectKey={projectKey}
            range={selectedRange}
            refreshIntervalMs={0}
            colSpan={8}
          />
        </tbody>
      </table>
    );
    const view = render(component("/repo"));
    act(() => shareCharts[0].onSelect?.("codex:p-1"));
    view.rerender(component("/other"));
    expect(useModelStatsMock).toHaveBeenLastCalledWith(
      range,
      expect.objectContaining({ projectKey: "/other", providerId: undefined }),
      expect.anything(),
    );
    act(() => shareCharts.at(-2)?.onSelect?.("codex:p-1"));
    view.rerender(component("/other", { preset: "7d" }));
    expect(useModelStatsMock).toHaveBeenLastCalledWith(
      { preset: "7d" },
      expect.objectContaining({ providerId: undefined }),
      expect.anything(),
    );
  });

  it("keeps every provider selectable in cache mode, including zero-input providers", async () => {
    useProviderStatsMock.mockReturnValue({
      data: Array.from({ length: 8 }, (_, index) => ({
        ...providerStat,
        providerId: `p-${index}`,
        freshInputTokens: 0,
        cacheReadTokens: 0,
      })),
    });
    render(
      <table>
        <tbody>
          <ProjectUsageBreakdown
            projectKey="/repo"
            range={range}
            refreshIntervalMs={0}
            colSpan={8}
          />
        </tbody>
      </table>,
    );
    await userEvent.click(
      screen.getByRole("button", { name: "usage.share.metric.cache" }),
    );
    const chart = shareCharts.filter((item) => item.onSelect).at(-1);
    expect(chart?.data).toHaveLength(8);
    expect(chart?.includeZeroValues).toBe(true);
    expect(chart?.maxSlices).toBe(8);
  });
});
