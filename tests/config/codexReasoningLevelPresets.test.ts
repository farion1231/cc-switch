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
  // 每条期望值都对应官方文档证据（见预设文件内注释）；改动任一侧前先核对来源。
  // 第四位=期望的显式 defaultReasoningLevel：仅在官方默认 ≠ 后端回落结果时
  // 声明（后端回落=模板默认 ∈ 子集则保留、否则取子集最高档），其余一律留空
  const EXPECTED: Array<[string, string, string[], string?]> = [
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
    // 腾讯 Token Plan（订阅线 /plan 端点）档位全部真 Key 实测（2026-08-31）：
    // glm-5.3 始终思考且档位严格枚举 low/high/max（medium/xhigh 直接 400，
    // 错误信息即枚举来源）；kimi-k2.7-code(-highspeed) 仅接受
    // thinking:enabled；minimax-m2.5/m2.7 与国内 auto 关思考被静默忽略
    //（选 none 是假关）→ 只列 high；其余模型 thinking 开关真实生效 → 两态
    ["Tencent Token Plan", "tc-code-latest", ["none", "high"]],
    ["Tencent Token Plan", "hy3", ["none", "high"]],
    ["Tencent Token Plan", "minimax-m2.7", ["high"]],
    [
      "Tencent Token Plan Enterprise Pro",
      "glm-5.3",
      ["low", "high", "max"],
      "high",
    ],
    ["Tencent Token Plan Enterprise Pro", "kimi-k2.7-code", ["high"]],
    ["Tencent Token Plan Enterprise Pro", "auto", ["high"]],
    ["Tencent Token Plan Enterprise Pro", "glm-5.2", ["none", "high"]],
    // 国际站 auto 尊重关思考（与国内 auto 忽略关思考行为不同）
    ["Tencent Token Plan (Intl)", "auto", ["none", "high"]],
    ["Tencent Token Plan Enterprise Pro (Intl)", "auto", ["none", "high"]],
    [
      "Tencent Token Plan Enterprise Pro (Intl)",
      "glm-5.3",
      ["low", "high", "max"],
      "high",
    ],
    ["Tencent Token Plan Enterprise Lite", "auto", ["high"]],
    ["Tencent Token Plan Enterprise Lite (Intl)", "auto", ["none", "high"]],
    // LongCat 无档位可调：全站唯一 effort 证据=官方示例的 high
    ["Longcat", "LongCat-2.0", ["high"]],
    // xAI Reasoning guide 模型级枚举；grok-4.5 不可关思考故无 none
    ["xAI (Grok)", "grok-4.5", ["low", "medium", "high", "xhigh"]],
    ["xAI (Grok) OAuth", "grok-4.5", ["low", "medium", "high", "xhigh"]],
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
    // 智谱官方 Codex 接入页自带 models.json（docs.bigmodel.cn/cn/coding-plan/tool/
    // codex、docs.z.ai/devpack/tool/codex，2026-09-04 核对）：glm-5.3 档位
    // low/high/max、默认 max（≠ 后端回落的模板默认 high，故显式声明）；
    // glm-5-turbo 官方档位为空、默认 max——cc-switch 表达不了空档位（会回落
    // 到模板 none/high，而 none 在原生直连下没有转换层兜底、会原样发给严格
    // 网关），按官方默认收成单档 max
    ["Zhipu GLM", "glm-5.3", ["low", "high", "max"], "max"],
    ["Zhipu GLM", "glm-5-turbo", ["max"]],
    ["Zhipu GLM en", "glm-5.3", ["low", "high", "max"], "max"],
    // SiliconFlow .com 的 M3：平台级 enable_thinking 布尔开关（后端按平台
    // 推断兜底），M3 官方可关思考 → 两态
    ["SiliconFlow en", "MiniMaxAI/MiniMax-M3", ["none", "high"]],
    // Novita：平台真开关 enable_thinking（声明已修正方言）→ 两态
    ["Novita AI", "zai-org/glm-5.1", ["none", "high"]],
    // 千帆 v2 官方 thinking:{type}（声明已补）→ 两态
    ["Baidu Qianfan Coding Plan", "qianfan-code-latest", ["none", "high"]],
    // 千帆 Token Plan：deepseek-v4-pro/v4-flash 在 thinking+reasoning_effort
    // 双官方清单内（effort 仅 high/max 两档真实深度）；不声明 default=回落
    // max，恰好等于平台对复杂 Agent 类请求的自动行为。glm-5.1 只在 thinking
    // 清单 → 两态
    ["Baidu Qianfan Token Plan", "deepseek-v4-pro", ["none", "high", "max"]],
    ["Baidu Qianfan Token Plan", "deepseek-v4-flash", ["none", "high", "max"]],
    ["Baidu Qianfan Token Plan", "glm-5.1", ["none", "high"]],
    // BytePlus 国际站已切原生 Responses，档位=官方 Codex 文档三档（与国内
    // 站火山双 Plan 同源交叉印证）
    ["BytePlus", "ark-code-latest", ["low", "medium", "high"]],
    // StepFun 官方两站模型页+reasoning 指南：3.7-flash 三档（默认 medium）、
    // 2603 两档；全系无关思考形态故无 none。effort 下发走后端 per-model
    // 推断（2603=low_high、3.7=passthrough）
    ["StepFun", "step-3.7-flash", ["low", "medium", "high"]],
    ["StepFun", "step-3.5-flash-2603", ["low", "high"]],
    ["StepFun en", "step-3.7-flash", ["low", "medium", "high"]],
    ["StepFun en", "step-3.5-flash-2603", ["low", "high"]],
    // Kimi 开放平台：k2.7-code 始终思考且官方标注不支持 effort → 单档；k3
    // 三档（官方默认 max=后端回落结果，无需显式 default）。均关不掉思考无 none
    ["Kimi", "kimi-k2.7-code", ["high"]],
    ["Kimi", "kimi-k3", ["low", "high", "max"]],
    // Kimi Code 端点：k3/k3-256k 官方默认 high ≠ 回落值 max → 显式 default
    // 首两例；kimi-for-coding(-highspeed) Thinking 恒 ON 单档
    ["Kimi For Coding", "kimi-for-coding", ["high"]],
    ["Kimi For Coding", "kimi-for-coding-highspeed", ["high"]],
    ["Kimi For Coding", "k3", ["low", "high", "max"], "high"],
    ["Kimi For Coding", "k3-256k", ["low", "high", "max"], "high"],
    // OpenCode Go（Zen 网关）：opencode 客户端 variants() 严格按各模型在
    // models.dev 的 reasoning_options 声明发 reasoning_effort（provider/
    // transform.ts）——glm-5.2 / deepseek-v4-pro 仅 high|max、v4-flash 另
    // 声明 low（均无 none：effort 档位非思考开关，none 会是假开关）。本 PR
    // 已为该预设补上 codexChatReasoning（effortValueMode:"zen"），代理转换
    // 层按同一张表逐模型钳制，表即下发生效的依据
    ["OpenCode Go", "glm-5.2", ["high", "max"]],
    ["OpenCode Go", "deepseek-v4-pro", ["high", "max"]],
    ["OpenCode Go", "deepseek-v4-flash", ["low", "high", "max"]],
  ];

  it.each(EXPECTED)(
    "%s / %s declares the vendor-documented levels",
    (presetName, modelId, levels, expectedDefault) => {
      const model = catalogModel(presetName, modelId);
      expect(model.reasoningLevels).toEqual(levels);
      // 默认档通常不显式声明（后端 fallback 已落到正确档位）；仅当官方默认
      // 与回落结果不一致时才有第四位期望值（Kimi Code k3 系先例）
      expect(model.defaultReasoningLevel).toBe(expectedDefault);
    },
  );

  it("keeps deliberately-unfilled presets unfilled", () => {
    // Bailian qwen3-coder-plus 无 per-model 档位证据。OpenCode Go 的
    // toggle/未收录模型保持不填：glm-5.1 是 toggle 型（models.dev 无 effort
    // 声明）、kimi-k2.7-code 官方标注不支持 effort、mimo-v2.5-pro 未收录
    // models.dev——与 opencode 客户端一致（代理侧无表不发 reasoning_effort
    // 字段）。SiliconFlow .cn 的 M2.5 能否真正关思考无官方明文、ModelScope
    // 是否透传思考字段未证实——真机验证前不造两态假开关（2026-08-15 盘点结论）
    const UNFILLED: Array<[string, string]> = [
      ["Bailian", "qwen3-coder-plus"],
      ["OpenCode Go", "glm-5.1"],
      ["OpenCode Go", "kimi-k2.7-code"],
      ["OpenCode Go", "mimo-v2.5-pro"],
      ["SiliconFlow", "Pro/MiniMaxAI/MiniMax-M2.5"],
      ["ModelScope", "ZhipuAI/GLM-5.2"],
      // StepFun 无后缀 3.5-flash：官方未暴露 effort，单一常开思考态
      ["StepFun", "step-3.5-flash"],
      ["StepFun en", "step-3.5-flash"],
      // Nvidia NIM：无思考开关（真参数 chat_template_kwargs 不在值域），
      // 声明已改 thinkingParam:none 撤销假开关
      ["Nvidia", "moonshotai/kimi-k2.5"],
      // 千帆 Token Plan：三模型均不在 thinking 官方清单（2026-05-27 版）且
      // 无任何官方接入示例下发思考字段——无证据不造档位
      ["Baidu Qianfan Token Plan", "deepseek-v4-flash-0731"],
      ["Baidu Qianfan Token Plan", "glm-5.2"],
      ["Baidu Qianfan Token Plan", "kimi-k2.6"],
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
