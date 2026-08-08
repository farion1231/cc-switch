import { render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageHero } from "@/components/usage/UsageHero";
import type { UsageRangeSelection } from "@/types/usage";

const useUsageSummaryByAppMock = vi.hoisted(() => vi.fn());
const usageHeatmapPropsMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/query/usage", () => ({
  useUsageSummaryByApp: (...args: unknown[]) =>
    useUsageSummaryByAppMock(...args),
}));

vi.mock("@/components/usage/UsageHeatmap", () => ({
  UsageHeatmap: (props: unknown) => {
    usageHeatmapPropsMock(props);
    return <div data-testid="usage-heatmap" />;
  },
}));

vi.mock("framer-motion", () => ({
  motion: {
    div: ({ children, ...props }: any) => <div {...props}>{children}</div>,
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

const range: UsageRangeSelection = { preset: "7d" };

describe("UsageHero heatmap integration", () => {
  beforeEach(() => {
    usageHeatmapPropsMock.mockReset();
    useUsageSummaryByAppMock.mockReset();
    useUsageSummaryByAppMock.mockReturnValue({ data: [], isLoading: false });
  });

  it("renders summary and heatmap inside one card with the same scope", () => {
    render(
      <UsageHero
        range={range}
        appType="codex"
        providerName="Provider A"
        model="gpt-5"
        refreshIntervalMs={5000}
      />,
    );

    const heatmap = screen.getByTestId("usage-heatmap");
    const card = heatmap.closest(".rounded-lg");
    expect(card).not.toBeNull();
    expect(
      within(card as HTMLElement).getByText("usage.realTotal"),
    ).toBeInTheDocument();
    expect(usageHeatmapPropsMock).toHaveBeenCalledWith({
      range,
      appType: "codex",
      providerName: "Provider A",
      model: "gpt-5",
      refreshIntervalMs: 5000,
    });
  });

  it("keeps the heatmap mounted while the summary is loading", () => {
    useUsageSummaryByAppMock.mockReturnValue({
      data: undefined,
      isLoading: true,
    });

    render(<UsageHero range={range} refreshIntervalMs={0} />);

    expect(screen.getByTestId("usage-heatmap")).toBeInTheDocument();
    expect(document.querySelector(".animate-spin")).toBeInTheDocument();
  });
});
