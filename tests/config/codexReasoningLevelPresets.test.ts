import { describe, expect, it } from "vitest";
import { codexProviderPresets } from "@/config/codexProviderPresets";

// 预填口径（2026-08-15 官方文档盘点 + Jason 同日拍板"表单可见性优先"）：
// - native Responses 直连预设：填厂商官方声明的真实差异化档位子集（含照抄
//   DeepSeek 官方 catalog 镜像、MiniMax/MiMo 与模板默认相同的显式声明——
//   表单显示"未设置"的误导比快照过时/冗余声明的代价更大）；
// - Chat 路由预设（supportsEffort:false）：档位值不进 wire，仅当预设声明了
//   真实思考开关（supportsThinking + thinkingParam）时填两态 none/high；
// - 后端对两条路径的 catalog 都应用 per-row 覆盖（apply_codex_reasoning_
//   level_override "Applies to every profile"）。
// 后端 codex_canonical_efforts 对未知值静默丢弃——预设里的拼写错误不会报错，
// 只会让 Codex 选择器静默少档/错档，所以白名单校验必须在测试层兜住。
const CANONICAL_EFFORTS = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
];

function catalogModel(presetName: string, modelId: string) {
  const preset = codexProviderPresets.find((item) => item.name === presetName);
  expect(preset, `preset ${presetName}`).toBeDefined();
  const model = (preset?.modelCatalog ?? []).find(
    (item) => item.model === modelId,
  );
  expect(model, `${presetName} catalog model ${modelId}`).toBeDefined();
  return model!;
}

describe("Codex preset pre-filled reasoning levels", () => {
  // 每条期望值都对应官方文档证据（见预设文件内注释）；改动任一侧前先核对来源
  const EXPECTED: Array<[string, string, string[]]> = [
    // 火山官方 Codex 接入文档四份一致：low/medium/high
    ["火山 Agent Plan", "ark-code-latest", ["low", "medium", "high"]],
    ["火山 Coding Plan", "ark-code-latest", ["low", "medium", "high"]],
    // 方舟深度思考文档：本模型无限制的通用四档（minimal=关思考直接回答）
    [
      "DouBaoSeed",
      "doubao-seed-2-1-pro-260628",
      ["minimal", "low", "medium", "high"],
    ],
    // 混元官方枚举 low/high；hy3 开源 chat template 对其他值直接 raise
    ["Tencent Hunyuan", "hy3", ["low", "high"]],
    ["Tencent Hunyuan", "hy3-preview", ["low", "high"]],
    // LongCat 无档位可调：全站唯一 effort 证据=官方示例的 high
    ["Longcat", "LongCat-2.0", ["high"]],
    // xAI Reasoning guide 模型级枚举；grok-4.5 不可关思考故无 none
    ["xAI (Grok)", "grok-4.5", ["low", "medium", "high"]],
    ["xAI (Grok) OAuth", "grok-4.5", ["low", "medium", "high"]],
    // DeepSeek 直连照抄官方 catalog 镜像（Jason 2026-08-15 拍板：表单可见性
    // 优先，接受快照过时风险——官方目录变更时须同步）
    ["DeepSeek", "deepseek-v4-flash", ["low", "high", "max"]],
    ["DeepSeek", "deepseek-v4-pro", ["low", "high", "max"]],
    // MiniMax/MiMo 官方 catalog=none/high（与模板默认一致，声明只为表单可见）
    ["MiniMax", "MiniMax-M3", ["none", "high"]],
    ["MiniMax en", "MiniMax-M3", ["none", "high"]],
    ["Xiaomi MiMo", "mimo-v2.5-pro", ["none", "high"]],
    ["Xiaomi MiMo", "mimo-v2.5", ["none", "high"]],
    ["Xiaomi MiMo Token Plan (China)", "mimo-v2.5-pro", ["none", "high"]],
    ["Xiaomi MiMo Token Plan (China)", "mimo-v2.5", ["none", "high"]],
    // GLM 走 Chat 路由（supportsEffort:false）：none=真实关思考开关，其余档
    // 等价开思考；只暴露两态，顺带补上模板四档里缺失的 none（关思考入口）
    ["Zhipu GLM", "glm-5.2", ["none", "high"]],
    ["Zhipu GLM en", "glm-5.2", ["none", "high"]],
    // SiliconFlow .com 的 M3：平台级 enable_thinking 布尔开关（后端按平台
    // 推断兜底），M3 官方可关思考 → 两态
    ["SiliconFlow en", "MiniMaxAI/MiniMax-M3", ["none", "high"]],
    // Novita：平台真开关 enable_thinking（声明已修正方言）→ 两态
    ["Novita AI", "zai-org/glm-5.1", ["none", "high"]],
    // StepFun 官方两站模型页+reasoning 指南：3.7-flash 三档（默认 medium）、
    // 2603 两档；全系无关思考形态故无 none。effort 下发走后端 per-model
    // 推断（2603=low_high、3.7=passthrough）
    ["StepFun", "step-3.7-flash", ["low", "medium", "high"]],
    ["StepFun", "step-3.5-flash-2603", ["low", "high"]],
    ["StepFun en", "step-3.7-flash", ["low", "medium", "high"]],
    ["StepFun en", "step-3.5-flash-2603", ["low", "high"]],
  ];

  it.each(EXPECTED)(
    "%s / %s declares the vendor-documented levels",
    (presetName, modelId, levels) => {
      const model = catalogModel(presetName, modelId);
      expect(model.reasoningLevels).toEqual(levels);
      // 默认档一律不显式声明：后端 fallback（模板默认 high ∈ 声明子集时保留）
      // 在上述每一家都自然落到正确的 high
      expect(model.defaultReasoningLevel).toBeUndefined();
    },
  );

  it("keeps deliberately-unfilled presets unfilled", () => {
    // Bailian qwen3-coder-plus 无 per-model 档位证据；OpenCode Go 是多厂商
    // Chat 网关且无 codexChatReasoning 声明（思考开关不生效，填 none 会是假
    // 开关），逐模型语义未经网关验证前保持不填。SiliconFlow .cn 的 M2.5 能否
    // 真正关思考无官方明文、ModelScope 是否透传思考字段未证实——真机验证前
    // 不造两态假开关（2026-08-15 盘点结论）
    const UNFILLED: Array<[string, string]> = [
      ["Bailian", "qwen3-coder-plus"],
      ["OpenCode Go", "glm-5.2"],
      ["OpenCode Go", "deepseek-v4-flash"],
      ["OpenCode Go", "mimo-v2.5-pro"],
      ["SiliconFlow", "Pro/MiniMaxAI/MiniMax-M2.5"],
      ["ModelScope", "ZhipuAI/GLM-5.2"],
      // StepFun 无后缀 3.5-flash：官方未暴露 effort，单一常开思考态
      ["StepFun", "step-3.5-flash"],
      ["StepFun en", "step-3.5-flash"],
      // Nvidia NIM：无思考开关（真参数 chat_template_kwargs 不在值域），
      // 声明已改 thinkingParam:none 撤销假开关
      ["Nvidia", "moonshotai/kimi-k2.5"],
    ];
    for (const [presetName, modelId] of UNFILLED) {
      const model = catalogModel(presetName, modelId);
      expect(
        model.reasoningLevels,
        `${presetName}/${modelId} must stay unfilled`,
      ).toBeUndefined();
    }
  });

  it("only ever declares canonical Codex efforts", () => {
    for (const preset of codexProviderPresets) {
      for (const model of preset.modelCatalog ?? []) {
        for (const level of model.reasoningLevels ?? []) {
          expect(
            CANONICAL_EFFORTS,
            `${preset.name}/${model.model} level "${level}"`,
          ).toContain(level);
        }
        if (model.defaultReasoningLevel !== undefined) {
          expect(model.reasoningLevels ?? []).toContain(
            model.defaultReasoningLevel,
          );
        }
      }
    }
  });
});
