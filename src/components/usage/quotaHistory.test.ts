import { describe, it, expect } from "vitest";
import type { SubscriptionQuota } from "@/types/subscription";
import type { QuotaHistoryRow } from "@/lib/api/quotaHistory";
import {
  appsWithHistory,
  hourIndex,
  hourStartMs,
  rowsToSeries,
  toQuotaSamples,
  usdMaxKey,
  usdUsedKey,
} from "./quotaHistory";

const HOUR = 3_600_000;

function quota(
  tiers: Array<{
    name: string;
    utilization: number;
    usedValueUsd?: number | null;
    maxValueUsd?: number | null;
  }>,
  queriedAt: number | null,
  success = true,
): SubscriptionQuota {
  return {
    tool: "claude",
    credentialStatus: "valid",
    credentialMessage: null,
    success,
    tiers: tiers.map((t) => ({ resetsAt: null, ...t })),
    extraUsage: null,
    error: null,
    queriedAt,
  };
}

function row(
  hour: number,
  tier: string,
  utilization: number,
  extra: Partial<QuotaHistoryRow> = {},
): QuotaHistoryRow {
  return {
    appId: "claude",
    hour,
    tier,
    utilization,
    usedUsd: null,
    maxUsd: null,
    ...extra,
  };
}

describe("hourIndex", () => {
  it("floors to the epoch hour, including before the epoch", () => {
    expect(hourIndex(10 * HOUR + 59 * 60_000)).toBe(10);
    expect(hourStartMs(hourIndex(10 * HOUR + 59 * 60_000))).toBe(10 * HOUR);
    expect(hourIndex(-1)).toBe(-1);
  });
});

describe("toQuotaSamples", () => {
  it("stamps the sample with the measurement time, not the wall clock", () => {
    const measured = 100 * HOUR + 12 * 60_000;
    const sample = toQuotaSamples(
      quota([{ name: "five_hour", utilization: 42.5 }], measured),
      500 * HOUR,
    );
    expect(sample).not.toBeNull();
    expect(sample!.measuredAt).toBe(measured);
    expect(sample!.tiers).toEqual([
      { name: "five_hour", utilization: 42.5, usedUsd: null, maxUsd: null },
    ]);
  });

  it("carries the dollar figures when the provider exposes them", () => {
    const sample = toQuotaSamples(
      quota(
        [
          {
            name: "seven_day",
            utilization: 10,
            usedValueUsd: 12.34,
            maxValueUsd: 200,
          },
        ],
        HOUR,
      ),
      HOUR,
    );
    expect(sample!.tiers[0]).toEqual({
      name: "seven_day",
      utilization: 10,
      usedUsd: 12.34,
      maxUsd: 200,
    });
  });

  it("falls back to now when the reading has no queriedAt", () => {
    const sample = toQuotaSamples(
      quota([{ name: "five_hour", utilization: 5 }], null),
      7 * HOUR,
    );
    expect(sample!.measuredAt).toBe(7 * HOUR);
  });

  it("records nothing for failed, empty or unusable readings", () => {
    expect(toQuotaSamples(undefined, HOUR)).toBeNull();
    expect(
      toQuotaSamples(
        quota([{ name: "five_hour", utilization: 5 }], HOUR, false),
        HOUR,
      ),
    ).toBeNull();
    expect(toQuotaSamples(quota([], HOUR), HOUR)).toBeNull();
    expect(
      toQuotaSamples(
        quota([{ name: "five_hour", utilization: NaN }], HOUR),
        HOUR,
      ),
    ).toBeNull();
  });

  it("drops unusable tiers but keeps the usable ones", () => {
    const sample = toQuotaSamples(
      quota(
        [
          { name: "five_hour", utilization: -1 },
          { name: "seven_day", utilization: 20 },
        ],
        HOUR,
      ),
      HOUR,
    );
    expect(sample!.tiers.map((t) => t.name)).toEqual(["seven_day"]);
  });
});

describe("rowsToSeries", () => {
  it("groups rows per hour and flattens tiers into chart rows", () => {
    const { tiers, rows } = rowsToSeries(
      [
        row(10, "five_hour", 20, { usedUsd: 3, maxUsd: 30 }),
        row(10, "seven_day", 40),
        row(11, "five_hour", 25),
      ],
      "claude",
    );

    expect(tiers).toEqual(["five_hour", "seven_day"]);
    expect(rows).toHaveLength(2);
    expect(rows[0].ts).toBe(hourStartMs(10));
    expect(rows[0][usdUsedKey("five_hour")]).toBe(3);
    expect(rows[0][usdMaxKey("five_hour")]).toBe(30);
    // A tier missing from an hour must be an explicit null, otherwise the chart
    // carries the previous value forward as if the quota had not moved.
    expect(rows[1].seven_day).toBeNull();
  });

  it("inserts a null row so long gaps break the line", () => {
    const { rows } = rowsToSeries(
      [row(10, "five_hour", 20), row(20, "five_hour", 60)],
      "claude",
    );
    expect(rows).toHaveLength(3);
    expect(rows[1].ts).toBe(hourStartMs(11));
    expect(rows[1].five_hour).toBeNull();
  });

  it("keeps consecutive hours connected", () => {
    const { rows } = rowsToSeries(
      [
        row(10, "five_hour", 20),
        row(11, "five_hour", 30),
        row(12, "five_hour", 40),
      ],
      "claude",
    );
    expect(rows).toHaveLength(3);
    expect(rows.every((r) => r.five_hour != null)).toBe(true);
  });

  it("only reads the requested app", () => {
    const rows = [
      row(10, "five_hour", 20),
      row(10, "premium", 90, { appId: "codex" }),
    ];
    expect(rowsToSeries(rows, "claude").tiers).toEqual(["five_hour"]);
    expect(rowsToSeries(rows, "codex").tiers).toEqual(["premium"]);
    expect(rowsToSeries(rows, "gemini").rows).toEqual([]);
  });

  it("sorts hours that arrive out of order", () => {
    const { rows } = rowsToSeries(
      [row(12, "five_hour", 40), row(11, "five_hour", 30)],
      "claude",
    );
    expect(rows.map((r) => r.ts)).toEqual([hourStartMs(11), hourStartMs(12)]);
  });
});

describe("appsWithHistory", () => {
  it("keeps the candidate order and skips apps without rows", () => {
    const rows = [row(10, "five_hour", 20, { appId: "codex" })];
    expect(appsWithHistory(rows, ["claude", "codex", "gemini"])).toEqual([
      "codex",
    ]);
    expect(appsWithHistory([], ["claude", "codex"])).toEqual([]);
  });
});
