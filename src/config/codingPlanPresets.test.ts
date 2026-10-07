import { describe, expect, it } from "vitest";
import { CODING_PLAN_CONCURRENCY_PRESETS } from "./codingPlanPresets";

describe("CODING_PLAN_CONCURRENCY_PRESETS", () => {
  it("Kimi 五档并发值为 1/1/2/2/8（设计决策 5，用户指定 2026-10-07）", () => {
    const kimi = CODING_PLAN_CONCURRENCY_PRESETS.filter((p) =>
      p.id.startsWith("kimi_"),
    );
    expect(kimi.map((p) => p.maxConcurrentRequests)).toEqual([1, 1, 2, 2, 8]);
  });

  it("GLM Max 默认 30，glm-5.3-flash 覆盖 50（控制台实测 2026-10-07）", () => {
    const glm = CODING_PLAN_CONCURRENCY_PRESETS.find((p) => p.id === "glm_max");
    expect(glm?.maxConcurrentRequests).toBe(30);
    expect(glm?.maxConcurrentByModel).toEqual({ "glm-5.3-flash": 50 });
  });

  it("MiMo 官网无数字，留空不限", () => {
    const mimo = CODING_PLAN_CONCURRENCY_PRESETS.find((p) => p.id === "mimo");
    expect(mimo?.maxConcurrentRequests).toBeUndefined();
    expect(mimo?.maxConcurrentByModel).toBeUndefined();
  });

  it("每条预设都带来源标注 key（tooltip 用）", () => {
    for (const preset of CODING_PLAN_CONCURRENCY_PRESETS) {
      expect(preset.sourceKey).toMatch(/^codingPlanPresets\.sources\./);
      expect(preset.labelKey).toMatch(/^codingPlanPresets\./);
    }
  });
});
