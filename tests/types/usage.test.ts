import { describe, expect, it } from "vitest";
import { getCacheWriteAvailability, omitsCacheWrites } from "@/types/usage";

describe("getCacheWriteAvailability", () => {
  it("distinguishes cache-write support across fixed protocols", () => {
    expect(getCacheWriteAvailability(["claude"])).toBe("ok");
    expect(getCacheWriteAvailability(["codex"])).toBe("ok");
    expect(getCacheWriteAvailability(["claude", "codex"])).toBe("ok");
    expect(getCacheWriteAvailability(["pi"])).toBe("partial");
    expect(getCacheWriteAvailability(["codex", "gemini"])).toBe("partial");
    expect(getCacheWriteAvailability(["gemini", "grokbuild"])).toBe("na");
    expect(getCacheWriteAvailability([])).toBe("ok");
  });

  it("treats a zero cache write as unreported only for Gemini and Grok Build", () => {
    expect(omitsCacheWrites("codex")).toBe(false);
    expect(omitsCacheWrites("claude")).toBe(false);
    expect(omitsCacheWrites("gemini")).toBe(true);
    expect(omitsCacheWrites("grokbuild")).toBe(true);
  });
});
