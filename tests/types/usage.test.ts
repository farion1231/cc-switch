import { describe, expect, it } from "vitest";
import { getCacheWriteAvailability } from "@/types/usage";

describe("getCacheWriteAvailability", () => {
  it("does not present Pi mixed-protocol cache creation as an authoritative zero", () => {
    expect(getCacheWriteAvailability(["pi"])).toBe("partial");
    expect(getCacheWriteAvailability(["pi", "codex"])).toBe("partial");
    expect(getCacheWriteAvailability(["pi", "claude"])).toBe("partial");
  });

  it("preserves fixed-protocol and cross-app availability states", () => {
    expect(getCacheWriteAvailability(["claude"])).toBe("ok");
    expect(getCacheWriteAvailability(["codex", "gemini"])).toBe("na");
    expect(getCacheWriteAvailability(["claude", "codex"])).toBe("partial");
    expect(getCacheWriteAvailability([])).toBe("ok");
  });
});
