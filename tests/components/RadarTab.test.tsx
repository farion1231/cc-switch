import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { RadarTab } from "@/components/codex-workbench/RadarTab";

vi.mock("@/lib/query/codexWorkbench", () => ({
  useCodexRadarQuery: () => ({
    data: {
      fromCache: true,
      stale: false,
      error: null,
      snapshot: {
        fetchedAt: 1,
        sourceUrl: "https://example.com/radar",
        models: [],
        comparisons: [],
      },
    },
    error: null,
    isError: false,
    isFetching: false,
    isLoading: false,
  }),
  useRefreshCodexRadar: () => ({
    mutate: vi.fn(),
    isPending: false,
  }),
}));

describe("RadarTab", () => {
  it("shows when radar data came from cache", () => {
    render(<RadarTab />);

    expect(
      screen.getByText(/codexWorkbench\.radar\.cache|缓存/),
    ).toBeInTheDocument();
  });
});
