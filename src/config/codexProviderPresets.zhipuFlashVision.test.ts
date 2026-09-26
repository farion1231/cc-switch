import { describe, expect, it } from "vitest";

import { codexProviderPresets } from "./codexProviderPresets";

// 回归：Zhipu GLM 官方 Codex 目录已加入 glm-5.3-flash（真机 /api/v1 Responses
// 实测支持图片输入）。预设必须显式声明 ["text", "image"]——否则用户以 flash
// 建行时会从 glm-5.3 行继承 ["text"] 显式声明，压掉 model_capabilities 的
// fail-open，Codex 端会直接拦截图片输入。
describe("Zhipu GLM preset carries the vision-capable glm-5.3-flash", () => {
  const preset = codexProviderPresets.find((p) => p.name === "Zhipu GLM");

  it("includes glm-5.3-flash with image input", () => {
    expect(preset).toBeDefined();
    const row = preset!.modelCatalog?.find((r) => r.model === "glm-5.3-flash");
    expect(row?.inputModalities).toEqual(["text", "image"]);
    expect(row?.contextWindow).toBe(1048576);
    expect(row?.reasoningLevels).toEqual(["low", "high", "max"]);
    expect(row?.supportsParallelToolCalls).toBe(true);
  });

  it("keeps the text-only glm-5.3 declaration explicit", () => {
    expect(
      preset!.modelCatalog?.find((r) => r.model === "glm-5.3")?.inputModalities,
    ).toEqual(["text"]);
  });
});
