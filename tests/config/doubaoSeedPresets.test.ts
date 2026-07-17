import { describe, expect, it } from "vitest";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";

// 回归：同一个 doubao 模型在 Codex catalog 与 OpenClaw settingsConfig 里都声明了
// contextWindow，二者必须一致。曾出现 OpenClaw 写成 128000、Codex 写成 262144 的
// 漂移，导致 OpenClaw 用户拿到过小的上下文窗口、长上下文提前压缩/截断。
describe("DouBaoSeed preset consistency across apps", () => {
  const DOUBAO_MODEL_ID = "doubao-seed-2-1-pro-260628";
  const EXPECTED_CONTEXT_WINDOW = 262144;

  it("keeps the doubao context window in sync between OpenClaw and Codex", () => {
    const codexPreset = codexProviderPresets.find(
      (item) => item.name === "DouBaoSeed",
    );
    const codexModel = (codexPreset?.modelCatalog ?? []).find(
      (model) => model.model === DOUBAO_MODEL_ID,
    );
    expect(codexModel, "Codex DouBaoSeed catalog model").toBeDefined();
    expect(codexModel?.contextWindow).toBe(EXPECTED_CONTEXT_WINDOW);

    const openclawPreset = openclawProviderPresets.find(
      (item) => item.name === "DouBaoSeed",
    );
    const openclawModel = (openclawPreset?.settingsConfig.models ?? []).find(
      (model) => model.id === DOUBAO_MODEL_ID,
    );
    expect(openclawModel, "OpenClaw DouBaoSeed model").toBeDefined();
    expect(openclawModel?.contextWindow).toBe(EXPECTED_CONTEXT_WINDOW);

    // 任一侧单独改动都会破坏这条等式 —— 这是真正防漂移的断言。
    expect(openclawModel?.contextWindow).toBe(codexModel?.contextWindow);
  });
});
