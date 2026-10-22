import { describe, expect, it } from "vitest";
import { formatReasoningLevelsLabel } from "@/components/providers/forms/CodexFormFields";

describe("formatReasoningLevelsLabel", () => {
  it("collapses a run of three or more adjacent levels into a range", () => {
    expect(
      formatReasoningLevelsLabel(["low", "medium", "high", "xhigh", "max"]),
    ).toBe("low → max");
  });

  it("collapses each run separately, so a skipped level stays visible", () => {
    // minimal sits between none and low: none must not join the low run.
    expect(
      formatReasoningLevelsLabel([
        "none",
        "low",
        "medium",
        "high",
        "xhigh",
        "max",
      ]),
    ).toBe("none, low → max");
    expect(formatReasoningLevelsLabel(["none", "low", "medium", "high"])).toBe(
      "none, low → high",
    );
  });

  it("lists short runs and gapped picks as they are", () => {
    expect(formatReasoningLevelsLabel(["high"])).toBe("high");
    expect(formatReasoningLevelsLabel(["none", "high"])).toBe("none, high");
    expect(formatReasoningLevelsLabel(["low", "high", "max"])).toBe(
      "low, high, max",
    );
    expect(formatReasoningLevelsLabel(["low", "medium", "xhigh"])).toBe(
      "low, medium, xhigh",
    );
  });

  it("keeps a non-canonical order as a plain list", () => {
    expect(formatReasoningLevelsLabel(["high", "medium", "low"])).toBe(
      "high, medium, low",
    );
  });
});
