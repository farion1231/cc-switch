import { describe, expect, it } from "vitest";
import {
  formatTokensShort,
  getLocaleFromLanguage,
  resolveUsageCostStatus,
  usageSourceDimensionsForScope,
} from "@/components/usage/format";
import { createAgentUsageSourceDimension as sourceDimension } from "../fixtures/agentUsage";

describe("usage format helpers", () => {
  it("formats Traditional Chinese token units with Traditional characters", () => {
    expect(formatTokensShort(12_345, "zh-TW")).toBe("1.2 萬");
    expect(formatTokensShort(123_456_789, "zh-Hant", 2)).toBe("1.23 億");
  });

  it("resolves Traditional Chinese locale aliases", () => {
    expect(getLocaleFromLanguage("zh_TW")).toBe("zh-TW");
    expect(getLocaleFromLanguage("zh-HK")).toBe("zh-TW");
  });

  it("resolves cost quality within each usage scope", () => {
    const dimensions = [
      sourceDimension("claude", "claude_session", {
        costStatus: "estimated",
        isDescendant: false,
      }),
      sourceDimension("claude", "claude_session", {
        costStatus: "unavailable",
        isDescendant: true,
      }),
    ];

    expect(
      resolveUsageCostStatus(usageSourceDimensionsForScope(dimensions, false)),
    ).toBe("estimated");
    expect(
      resolveUsageCostStatus(usageSourceDimensionsForScope(dimensions, true)),
    ).toBe("unavailable");
    expect(resolveUsageCostStatus(dimensions)).toBe("unavailable");
  });
});
