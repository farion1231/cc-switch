import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { UsageHeatmap } from "@/components/usage/UsageHeatmap";

const trendsQuery = vi.hoisted(() => vi.fn());
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
vi.mock("@/lib/query/usage", () => ({
  useUsageTrends: (...args: unknown[]) => trendsQuery(...args),
}));
afterEach(cleanup);

it("keeps Hermes filters and all token dimensions in the upstream heatmap", () => {
  trendsQuery.mockReturnValue({
    data: [
      {
        date: new Date().toISOString(),
        requestCount: 1,
        totalInputTokens: 10,
        totalOutputTokens: 5,
        totalCacheCreationTokens: 4,
        totalCacheReadTokens: 3,
        totalCacheWriteTokens: 6,
        totalReasoningTokens: 2,
        totalCost: "0.1",
      },
    ],
    isLoading: false,
  });
  const props = {
    appType: "hermes",
    profileName: "profile-a",
    task: "task-a",
    refreshIntervalMs: 0,
  };
  const { container } = render(<UsageHeatmap {...props} />);
  expect(trendsQuery).toHaveBeenLastCalledWith(
    expect.anything(),
    expect.objectContaining({ profileName: "profile-a", task: "task-a" }),
    expect.anything(),
  );
  const weekday = (new Date().getDay() + 6) % 7;
  const cell = container.querySelectorAll(".aspect-square")[weekday * 53 + 52];
  expect(cell).not.toBeNull();
  fireEvent.mouseEnter(cell!);
  expect(screen.getByRole("tooltip")).toHaveTextContent("30");
  expect(screen.getByRole("tooltip")).toHaveTextContent(
    "usage.countLabel.hermesApiCalls",
  );
});
