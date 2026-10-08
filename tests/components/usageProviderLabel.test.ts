import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import {
  getUsageProviderLabel,
  usageProviderTitle,
} from "@/components/usage/providerLabel";

const t = ((key: string, options?: Record<string, unknown>) =>
  options?.app ? `${key}:${options.app}` : key) as unknown as TFunction;

describe("getUsageProviderLabel", () => {
  it("translates session-log placeholder names with the full app name", () => {
    expect(getUsageProviderLabel("Claude (Session)", t)).toEqual({
      label: "usage.sessionProvider.label:Claude Code",
      shortLabel: "usage.sessionProvider.short",
      hint: "usage.sessionProvider.hint",
    });
    expect(getUsageProviderLabel("Gemini (Session)", t).label).toBe(
      "usage.sessionProvider.label:Gemini CLI",
    );
  });

  it("keeps real provider names as-is", () => {
    expect(getUsageProviderLabel("DeepSeek", t)).toEqual({
      label: "DeepSeek",
      shortLabel: "DeepSeek",
    });
  });

  it("falls back to unknown provider for empty names", () => {
    expect(getUsageProviderLabel("", t).label).toBe("usage.unknownProvider");
    expect(getUsageProviderLabel(undefined, t).label).toBe(
      "usage.unknownProvider",
    );
  });

  it("appends the hint to the hover title only for placeholders", () => {
    expect(usageProviderTitle({ label: "A", hint: "B" })).toBe("A\nB");
    expect(usageProviderTitle({ label: "A" })).toBe("A");
  });
});
