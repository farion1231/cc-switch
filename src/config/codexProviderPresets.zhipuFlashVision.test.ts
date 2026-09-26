import { describe, expect, it } from "vitest";

import { codexProviderPresets } from "./codexProviderPresets";

// 回归（#7688）：智谱国内站/国际站 Codex 预设都要自带视觉模型 glm-5.3-flash 行，
// 并显式声明 ["text", "image"]。官方 Codex models.json 尚未收录 flash，依据是
// 两站模型页（原生多模态、1M 上下文、Coding Plan 全量开放）。缺这一行时，用户
// 会把 glm-5.3 行改名成 flash，连带继承该行隐藏的 ["text"] 声明，Codex 端因此
// 直接拦截图片输入。
describe.each(["Zhipu GLM", "Zhipu GLM en"])(
  "%s preset carries the vision-capable glm-5.3-flash",
  (presetName) => {
    const preset = codexProviderPresets.find((p) => p.name === presetName);

    it("includes glm-5.3-flash with image input", () => {
      expect(preset).toBeDefined();
      const row = preset!.modelCatalog?.find(
        (r) => r.model === "glm-5.3-flash",
      );
      expect(row?.inputModalities).toEqual(["text", "image"]);
      expect(row?.contextWindow).toBe(1048576);
      expect(row?.reasoningLevels).toEqual(["low", "high", "max"]);
      expect(row?.defaultReasoningLevel).toBe("max");
      expect(row?.supportsParallelToolCalls).toBe(true);
    });

    it("keeps the text-only glm-5.3 declaration explicit", () => {
      expect(
        preset!.modelCatalog?.find((r) => r.model === "glm-5.3")
          ?.inputModalities,
      ).toEqual(["text"]);
    });
  },
);
