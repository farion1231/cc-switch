import { describe, expect, it } from "vitest";
import { calculateTokensProcessed } from "./UsageTrendChart";

describe("calculateTokensProcessed", () => {
  it("includes input, output, cache creation, and cache read tokens", () => {
    expect(
      calculateTokensProcessed({
        totalInputTokens: 47_431_464,
        totalOutputTokens: 3_780_479,
        totalCacheCreationTokens: 0,
        totalCacheReadTokens: 1_569_037_696,
      }),
    ).toBe(1_620_249_639);
  });
});
