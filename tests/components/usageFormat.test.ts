import { describe, expect, it } from "vitest";
import {
  formatUsageCost,
  formatTokensShort,
  getLocaleFromLanguage,
  resolveUsageCostStatus,
  resolveUsageCostStatusForMeasure,
  usageSourceDimensionsForScope,
} from "@/components/usage/format";
import {
  createAgentUsageMeasure as measure,
  createAgentUsageSourceDimension as sourceDimension,
} from "../fixtures/agentUsage";

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

  it("does not present partial or unknown costs as exact reported values", () => {
    const cost = measure({ totalCostUsd: "0.042" });

    for (const costStatus of ["partial", "unknown", null]) {
      const dimensions = [
        sourceDimension("grokbuild", "grok_session", { costStatus }),
      ];

      expect(resolveUsageCostStatusForMeasure(cost, dimensions)).toBe(
        "unavailable",
      );
      expect(formatUsageCost(cost, dimensions)).toBe("—");
    }

    for (const costStatus of ["actual", "complete", "reported"]) {
      expect(
        resolveUsageCostStatus([
          sourceDimension("codex", "codex_session", { costStatus }),
        ]),
      ).toBe("reported");
    }
  });
});
