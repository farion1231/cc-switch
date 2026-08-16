import { describe, expect, it } from "vitest";
import {
  fmtUsd,
  formatTokensShort,
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

  it("places the sign before the currency symbol for signed adjustments", () => {
    expect(fmtUsd(-1, 6)).toBe("-$1.000000");
  });

  it("preserves positive cost formatting and invalid fallbacks", () => {
    expect(fmtUsd(1, 2)).toBe("$1.00");
    expect(fmtUsd("not-a-number", 2)).toBe("--");
  });
});
