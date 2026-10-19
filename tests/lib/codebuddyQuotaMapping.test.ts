import { describe, expect, it } from "vitest";
import type { CodebuddyQuota } from "@/types/subscription";
import {
  earliestCycleEnd,
  formatCodebuddyCredits,
} from "@/components/CodebuddyOauthQuotaFooter";

type Package = CodebuddyQuota["packages"][number];

function pkg(overrides: Partial<Package>): Package {
  return {
    name: "CodeBuddy个人体验版",
    total: 500,
    remaining: 250,
    used: 250,
    cycle_start: null,
    cycle_end: null,
    ...overrides,
  };
}

describe("CodeBuddy provider-card quota", () => {
  it("formats credits with grouping and no trailing decimals", () => {
    expect(formatCodebuddyCredits(5500, "zh-CN")).toBe("5,500");
    expect(formatCodebuddyCredits(250000, "en-US")).toBe("250,000");
    expect(formatCodebuddyCredits(241463.66, "zh-CN")).toBe("241,463.66");
    expect(formatCodebuddyCredits(0, "zh-CN")).toBe("0");
  });

  it("uses the earliest package cycle end as the reset countdown target", () => {
    const resetAt = earliestCycleEnd([
      pkg({ cycle_end: "2026-11-30 23:59:59" }),
      pkg({ cycle_end: "2026-10-31 23:59:59" }),
      pkg({ cycle_end: null }),
    ]);
    expect(resetAt).toBe(new Date("2026-10-31T23:59:59").toISOString());
  });

  it("returns no reset time when no package has a parseable cycle end", () => {
    expect(
      earliestCycleEnd([pkg({ cycle_end: null }), pkg({ cycle_end: " " })]),
    ).toBeNull();
  });
});
