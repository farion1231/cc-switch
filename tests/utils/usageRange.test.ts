import { describe, expect, it } from "vitest";
import { getUsageRangePresetLabel, resolveUsageRange } from "@/lib/usageRange";

describe("usage range helpers", () => {
  it("leaves date bounds unset for the all preset", () => {
    expect(resolveUsageRange({ preset: "all" })).toEqual({});
  });

  it("labels the all preset through i18n", () => {
    const t = (key: string, options?: { defaultValue?: string }) =>
      key === "usage.presetAll" ? "All" : (options?.defaultValue ?? key);

    expect(getUsageRangePresetLabel("all", t)).toBe("All");
  });
});
