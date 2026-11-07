import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import { claudePlanLabel } from "@/lib/claudePlan";

// 测试里 i18n 用空资源：按 defaultValue 断言
const t = ((key: string, options?: { defaultValue?: string }) =>
  options?.defaultValue ?? key) as unknown as TFunction;

describe("claudePlanLabel", () => {
  it("names each plan, telling Max 5x from 20x", () => {
    expect(claudePlanLabel(t, "pro", null)).toBe("Pro");
    expect(claudePlanLabel(t, "max", "default_claude_max_20x")).toBe("Max 20x");
    expect(claudePlanLabel(t, "max", "default_claude_max_5x")).toBe("Max 5x");
    expect(claudePlanLabel(t, "max", null)).toBe("Max");
    expect(claudePlanLabel(t, "team", null)).toBe("Team");
    expect(claudePlanLabel(t, "team", "default_claude_max_5x")).toBe(
      "Team 高级席位",
    );
    expect(claudePlanLabel(t, "enterprise", "x")).toBe("Enterprise");
  });

  it("shows an unknown plan as is and nothing when there is none", () => {
    expect(claudePlanLabel(t, "ultra", null)).toBe("Ultra");
    expect(claudePlanLabel(t, null, "default_claude_max_20x")).toBeNull();
    expect(claudePlanLabel(t, " ", null)).toBeNull();
  });
});
