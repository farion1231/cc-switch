import { describe, expect, it } from "vitest";
import { normalizeClaudeChatReasoningForSave } from "@/components/providers/forms/ProviderForm";
import type { ClaudeChatReasoning } from "@/types";

describe("ProviderForm Claude reasoning helpers", () => {
  it("保存时过滤空值和非法映射", () => {
    const value = {
      effortMap: {
        low: "low",
        medium: undefined,
        high: "",
        unknown: "ultra",
        max: "max",
      },
    } as unknown as ClaudeChatReasoning;

    expect(normalizeClaudeChatReasoningForSave(value)).toEqual({
      effortMap: {
        low: "low",
        max: "max",
      },
    });
  });

  it("没有合法映射时不保存配置", () => {
    const value = {
      effortMap: {
        max: undefined,
      },
    } as unknown as ClaudeChatReasoning;

    expect(normalizeClaudeChatReasoningForSave(value)).toBeUndefined();
  });
});
