import { describe, expect, it } from "vitest";
import {
  buildModelDistribution,
  formatModelShare,
} from "@/components/usage/modelDistribution";
import type { ModelStats } from "@/types/usage";

function stat(
  model: string,
  totalTokens: number,
  totalCost: string,
): ModelStats {
  return {
    model,
    requestCount: 1,
    totalTokens,
    totalCost,
    avgCostPerRequest: totalCost,
  };
}

describe("model distribution", () => {
  it("uses the model table token and cost totals as separate share denominators", () => {
    const stats = [
      stat("luna", 48_027_912, "37.3162"),
      stat("sol", 13_397_326, "285.0980"),
      stat("astra", 3_167_578, "152.9118"),
      stat("terra", 515_797, "2.6810"),
    ];
    const distribution = buildModelDistribution(stats);

    expect(distribution.totalTokens).toBe(65_108_613);
    expect(distribution.totalCost).toBeCloseTo(478.007, 8);
    expect(distribution.rows.map((row) => row.model)).toEqual([
      "luna",
      "sol",
      "astra",
      "terra",
    ]);
    expect(distribution.rows[0].tokenShare).toBeCloseTo(
      48_027_912 / 65_108_613,
    );
    expect(distribution.rows[0].costShare).toBeCloseTo(37.3162 / 478.007);
    expect(distribution.rows[1].costShare).toBeCloseTo(285.098 / 478.007);
    expect(formatModelShare(distribution.rows[0].tokenShare)).toBe("73.8%");
    expect(formatModelShare(distribution.rows[0].costShare)).toBe("7.8%");
    expect(formatModelShare(distribution.rows[1].costShare)).toBe("59.6%");
    expect(distribution.invalidTokens).toBe(false);
    expect(distribution.invalidCost).toBe(false);
    expect(stats[0]).not.toHaveProperty("tokenShare");
  });

  it("keeps zero and empty metrics distinct", () => {
    expect(buildModelDistribution([])).toMatchObject({
      rows: [],
      totalTokens: 0,
      totalCost: 0,
      invalidTokens: false,
      invalidCost: false,
    });

    const distribution = buildModelDistribution([
      stat("paid", 10, "2"),
      stat("zero", 0, "0"),
    ]);
    expect(distribution.rows[1]).toMatchObject({
      tokenValue: 0,
      cost: 0,
      tokenShare: 0,
      costShare: 0,
    });
    expect(
      buildModelDistribution([stat("zero", 0, "0")]).rows[0],
    ).toMatchObject({
      tokenShare: null,
      costShare: null,
    });
  });

  it("excludes invalid metric values without changing source rows", () => {
    const invalid = stat("invalid", -5, "Infinity");
    const distribution = buildModelDistribution([
      invalid,
      stat("valid", 10, "2.5"),
    ]);
    expect(distribution).toMatchObject({
      totalTokens: 10,
      totalCost: 2.5,
      invalidTokens: true,
      invalidCost: true,
    });
    expect(distribution.rows[0]).toMatchObject({
      totalTokens: -5,
      totalCost: "Infinity",
      tokenValue: 0,
      cost: 0,
      tokenShare: null,
      costShare: null,
    });
    expect(distribution.rows[1]).toMatchObject({
      tokenShare: null,
      costShare: null,
    });
    expect(invalid.totalTokens).toBe(-5);
  });

  it("assigns each exact model name the same color after reorder and filtering", () => {
    const alpha = stat("alpha", 1, "1");
    const beta = stat("beta", 2, "2");
    const first = buildModelDistribution([alpha, beta]).rows[0].color;
    expect(buildModelDistribution([beta, alpha]).rows[1].color).toBe(first);
    expect(buildModelDistribution([alpha]).rows[0].color).toBe(first);
    expect(buildModelDistribution([beta]).rows[0].color).not.toBe(first);
  });

  it("formats zero, tiny and rounded shares", () => {
    expect(formatModelShare(null)).toBe("—");
    expect(formatModelShare(0)).toBe("0.0%");
    expect(formatModelShare(0.0009)).toBe("<0.1%");
    expect(formatModelShare(0.001)).toBe("0.1%");
    expect(formatModelShare(0.12345)).toBe("12.3%");
  });
});
