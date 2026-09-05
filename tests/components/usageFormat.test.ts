import { describe, expect, it } from "vitest";
import {
  formatTokensShort,
  fmtUsdPerMillion,
  getLocaleFromLanguage,
} from "@/components/usage/format";

describe("usage format helpers", () => {
  it("formats Traditional Chinese token units with Traditional characters", () => {
    expect(formatTokensShort(12_345, "zh-TW")).toBe("1.2 萬");
    expect(formatTokensShort(123_456_789, "zh-Hant", 2)).toBe("1.23 億");
  });

  it("resolves Traditional Chinese locale aliases", () => {
    expect(getLocaleFromLanguage("zh_TW")).toBe("zh-TW");
    expect(getLocaleFromLanguage("zh-HK")).toBe("zh-TW");
  });
});

describe("fmtUsdPerMillion", () => {
  it("uses total cost and cache-inclusive tokens rather than request count", () => {
    expect(fmtUsdPerMillion("12.5", 5_000_000)).toBe("$2.5000");
    expect(fmtUsdPerMillion("0", 1_000_000)).toBe("$0.0000");
  });
  it("does not present missing or zero-token usage as a free rate", () => {
    expect(fmtUsdPerMillion("12.5", 0)).toBe("--");
    expect(fmtUsdPerMillion("invalid", 1_000_000)).toBe("--");
    expect(fmtUsdPerMillion("12.5", Infinity)).toBe("--");
  });
});
