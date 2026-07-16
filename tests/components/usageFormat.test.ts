import { describe, expect, it } from "vitest";
import {
  formatReasoning,
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
});

describe("formatReasoning", () => {
  it.each([
    [undefined, 0, "not_attempted", "—"],
    [0, 0, "not_triggered", "Tok 0"],
    [500, 0, "not_triggered", "Tok 500"],
    [500, 2, "continued", "Tok 500 ✨2"],
    [500, 1, "partial_failed", "Tok 500 ⚠"],
  ] as const)(
    "renders reasoningTokens=%s rounds=%s status=%s as %s",
    (reasoningTokens, continuationRounds, continuationStatus, expected) => {
      expect(
        formatReasoning({
          reasoningTokens,
          continuationRounds,
          continuationStatus,
        }),
      ).toBe(expected);
    },
  );
});

