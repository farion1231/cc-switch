import { describe, expect, it } from "vitest";
import { getCacheWriteAvailability } from "@/types/usage";

describe("getCacheWriteAvailability", () => {
  it("distinguishes cache-write support across fixed protocols", () => {
    expect(getCacheWriteAvailability(["claude"])).toBe("ok");
    expect(getCacheWriteAvailability(["pi"])).toBe("partial");
    expect(getCacheWriteAvailability(["codex", "gemini"])).toBe("na");
    expect(getCacheWriteAvailability(["claude", "codex"])).toBe("partial");
    expect(getCacheWriteAvailability([])).toBe("ok");
  });

  it("treats workbuddy like other cache-inclusive app types", () => {
    expect(getCacheWriteAvailability(["workbuddy"])).toBe("na");
    expect(getCacheWriteAvailability(["claude", "workbuddy"])).toBe("partial");
  });
});
