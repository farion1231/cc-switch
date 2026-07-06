import { describe, expect, it } from "vitest";
import {
  buildActivityHeatmap,
  getActivityIntensity,
  parseLocalDateString,
} from "./activityHeatmap";

describe("activity heatmap helpers", () => {
  it("returns level 0 for empty usage and keeps dated cells in week rows", () => {
    const matrix = buildActivityHeatmap([
      {
        date: "2026-01-05",
        realTotalTokens: 0,
        sessionCount: 0,
        requestCount: 0,
        totalCost: "0",
      },
    ]);

    expect(matrix.weeks).toHaveLength(1);
    expect(matrix.weeks[0].cells[0]?.intensity).toBe(0);
    expect(matrix.activeDays).toBe(0);
    expect(matrix.totalTokens).toBe(0);
  });

  it("uses log scaling so a single extreme day does not flatten normal days", () => {
    const normalDay = {
      realTotalTokens: 10_000,
      sessionCount: 1,
    };

    expect(getActivityIntensity(normalDay, 1_000_000_000, 50)).toBeGreaterThan(
      1,
    );
    expect(
      getActivityIntensity(
        {
          realTotalTokens: 1_000_000_000,
          sessionCount: 50,
        },
        1_000_000_000,
        50,
      ),
    ).toBe(5);
  });

  it("lets session count contribute to the final intensity", () => {
    const lowSession = getActivityIntensity(
      { realTotalTokens: 1_000, sessionCount: 1 },
      10_000,
      100,
    );
    const highSession = getActivityIntensity(
      { realTotalTokens: 1_000, sessionCount: 100 },
      10_000,
      100,
    );

    expect(highSession).toBeGreaterThan(lowSession);
  });

  it("rejects invalid local date strings", () => {
    expect(parseLocalDateString("2026-02-29")).toBeNull();
    expect(parseLocalDateString("2026-02-28")?.getDate()).toBe(28);
  });
});
