import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { ProviderUsageBreakdown } from "@/components/usage/ProviderUsageBreakdown";
import type { UsageRangeSelection } from "@/types/usage";

const useModelStatsMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useModelStats: (...args: unknown[]) => useModelStatsMock(...args),
}));

vi.mock("@/components/usage/UsageShareChart", () => ({
  UsageShareChart: () => <div data-testid="share-chart" />,
}));

const range: UsageRangeSelection = { preset: "today" };

describe("ProviderUsageBreakdown", () => {
  beforeEach(() => {
    useModelStatsMock.mockReset();
    useModelStatsMock.mockReturnValue({ data: [] });
  });

  it("filters models by provider id and app type, not display name", () => {
    render(
      <table>
        <tbody>
          <ProviderUsageBreakdown
            providerId="p-1"
            providerAppType="codex"
            providerName="DeepSeek"
            projectKey="/repo"
            range={range}
            refreshIntervalMs={0}
            colSpan={8}
          />
        </tbody>
      </table>,
    );

    expect(useModelStatsMock).toHaveBeenCalledWith(
      range,
      { providerId: "p-1", providerAppType: "codex", projectKey: "/repo" },
      { refetchInterval: false },
    );
  });

  it("opens model cache composition without changing the provider scope", async () => {
    useModelStatsMock.mockReturnValue({
      data: [
        {
          model: "model-a",
          requestCount: 12,
          totalTokens: 2000,
          totalCost: "1",
          freshInputTokens: 100,
          cacheCreationTokens: 100,
          cacheReadTokens: 800,
          cacheableInputTokens: 1000,
          cacheHitRate: 0.8,
        },
      ],
    });
    render(
      <table>
        <tbody>
          <ProviderUsageBreakdown
            providerId="p-1"
            providerAppType="codex"
            providerName="DeepSeek"
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
    expect(screen.getByText("model-a")).toBeInTheDocument();
    expect(screen.getAllByText("80.0%").length).toBeGreaterThan(0);
    expect(screen.queryByTestId("share-chart")).not.toBeInTheDocument();
    expect(useModelStatsMock).toHaveBeenLastCalledWith(
      range,
      { providerId: "p-1", providerAppType: "codex", projectKey: "/repo" },
      { refetchInterval: false },
    );
  });

  it("hides previous-provider placeholder data while the new scope loads", () => {
    useModelStatsMock.mockReturnValue({
      data: [{ model: "old-provider-model" }],
      isPlaceholderData: true,
    });
    render(
      <table>
        <tbody>
          <ProviderUsageBreakdown
            providerId="p-2"
            providerAppType="codex"
            providerName="New provider"
            range={range}
            refreshIntervalMs={0}
            colSpan={8}
          />
        </tbody>
      </table>,
    );
    expect(screen.getByRole("status")).toHaveTextContent("common.loading");
    expect(screen.queryByTestId("share-chart")).not.toBeInTheDocument();
  });

  it("offers retry when loading fails without treating the error as empty data", async () => {
    const refetch = vi.fn();
    useModelStatsMock.mockReturnValue({ isError: true, refetch });
    render(
      <table>
        <tbody>
          <ProviderUsageBreakdown
            providerId="p-1"
            providerAppType="codex"
            providerName="DeepSeek"
            range={range}
            refreshIntervalMs={0}
            colSpan={8}
          />
        </tbody>
      </table>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "usage.cacheBreakdown.loadError",
    );
    await userEvent.click(screen.getByRole("button", { name: "common.retry" }));
    expect(refetch).toHaveBeenCalledOnce();
  });
});
