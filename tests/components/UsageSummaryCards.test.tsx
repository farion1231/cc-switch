import { render } from "@testing-library/react";
import "@testing-library/jest-dom";
import { describe, expect, it, vi } from "vitest";
import { UsageSummaryCards } from "@/components/usage/UsageSummaryCards";

const summary = {
  totalRequests: 42,
  totalCost: 1.2345,
  totalInputTokens: 1000,
  totalOutputTokens: 2000,
  totalCacheCreationTokens: 300,
  totalCacheReadTokens: 400,
};

const useUsageSummaryMock = vi.fn(
  (_days: number, _options?: { refetchInterval?: number | false }) => ({
    data: summary,
    isLoading: false,
  }),
);

const parseFiniteNumberMock = vi.fn((value: unknown): number | null =>
  typeof value === "number" ? value : null,
);
const fmtUsdMock = vi.fn((value: number, digits = 4) => `$${value.toFixed(digits)}`);

vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return {
    useTranslation: () => ({ t }),
  };
});

vi.mock("@/lib/query/usage", () => ({
  useUsageSummary: (days: number, options?: { refetchInterval?: number | false }) =>
    useUsageSummaryMock(days, options),
}));

vi.mock("@/components/usage/format", () => ({
  parseFiniteNumber: (value: unknown) => parseFiniteNumberMock(value),
  fmtUsd: (value: number, digits?: number) => fmtUsdMock(value, digits),
}));

vi.mock("framer-motion", () => ({
  motion: {
    div: ({
      children,
      ...props
    }: React.HTMLAttributes<HTMLDivElement> & { children: React.ReactNode }) => (
      <div {...props}>{children}</div>
    ),
  },
}));

vi.mock("@/components/ui/card", () => ({
  Card: ({
    children,
    ...props
  }: React.HTMLAttributes<HTMLDivElement> & { children: React.ReactNode }) => (
    <div {...props}>{children}</div>
  ),
  CardContent: ({
    children,
    ...props
  }: React.HTMLAttributes<HTMLDivElement> & { children: React.ReactNode }) => (
    <div {...props}>{children}</div>
  ),
}));

describe("UsageSummaryCards", () => {
  it("does not recompute summary stats on identical rerenders", () => {
    const { rerender } = render(
      <UsageSummaryCards days={7} refreshIntervalMs={0} />,
    );

    expect(parseFiniteNumberMock).toHaveBeenCalledTimes(1);
    expect(fmtUsdMock).toHaveBeenCalledTimes(1);

    rerender(<UsageSummaryCards days={7} refreshIntervalMs={0} />);

    expect(parseFiniteNumberMock).toHaveBeenCalledTimes(1);
    expect(fmtUsdMock).toHaveBeenCalledTimes(1);
  });
});
