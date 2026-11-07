import { describe, expect, it } from "vitest";
import { cacheHitLabel, getCacheStats } from "@/components/usage/cacheStats";

describe("cache token statistics", () => {
  it("includes writes and fresh input in the denominator", () => {
    expect(
      getCacheStats({
        freshInputTokens: 100,
        cacheCreationTokens: 100,
        cacheReadTokens: 800,
      }),
    ).toEqual({ fresh: 100, write: 100, read: 800, input: 1000, rate: 0.8 });
  });

  it("distinguishes no input, a zero hit rate and missing cache fields", () => {
    const zero = {
      freshInputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
    };
    expect(cacheHitLabel(zero)).toBe("—");
    expect(cacheHitLabel({ ...zero, freshInputTokens: 100 })).toBe("0.0%");
    expect(getCacheStats({ ...zero, cacheReadTokens: undefined })).toBeNull();
    expect(cacheHitLabel({ ...zero, cacheReadTokens: undefined })).toBe("—");
  });

  it.each([-1, Number.NaN, Number.POSITIVE_INFINITY])(
    "rejects invalid cache data: %s",
    (value) => {
      expect(
        getCacheStats({
          freshInputTokens: 10,
          cacheCreationTokens: 0,
          cacheReadTokens: value,
        }),
      ).toBeNull();
    },
  );
});
