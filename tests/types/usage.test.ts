import { describe, expect, it } from "vitest";
import { getCacheWriteAvailability, getFreshInputTokens } from "@/types/usage";

describe("getCacheWriteAvailability", () => {
  it("distinguishes cache-write support across fixed protocols", () => {
    expect(getCacheWriteAvailability(["claude"])).toBe("ok");
    expect(getCacheWriteAvailability(["pi"])).toBe("partial");
    expect(getCacheWriteAvailability(["codex", "gemini"])).toBe("na");
    expect(getCacheWriteAvailability(["claude", "codex"])).toBe("partial");
    expect(getCacheWriteAvailability(["copilot-cli"])).toBe("ok");
    expect(getCacheWriteAvailability([])).toBe("ok");
  });
});

describe("getFreshInputTokens", () => {
  it("removes Copilot CLI cache reads and writes from cumulative input", () => {
    expect(
      getFreshInputTokens({
        appType: "copilot-cli",
        inputTokens: 1_000_000,
        cacheReadTokens: 200_000,
        cacheCreationTokens: 100_000,
      }),
    ).toBe(700_000);
  });
});
