import { describe, expect, it } from "vitest";
import { codexProviderPresets } from "@/config/codexProviderPresets";

// 预填口径（2026-08-15 官方文档盘点 + Jason 同日拍板"表单可见性优先"）：
// - native Responses 直连预设：填厂商官方声明的真实差异化档位子集（含照抄
//   DeepSeek 官方 catalog 镜像、MiniMax/MiMo 与模板默认相同的显式声明——
//   表单显示"未设置"的误导比快照过时/冗余声明的代价更大）；
// - Chat 路由预设（supportsEffort:false）：档位值不进 wire，仅当预设声明了
//   真实思考开关（supportsThinking + thinkingParam）时填两态 none/high；
//   未确认开关且模型会思考时，仅列 high 表示单一思考模式；
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
  // 第四位=期望的显式 defaultReasoningLevel：用于官方 catalog 的默认值，
  // 或让 Chat catalog 与预设 config.toml 的显式 high 一致（否则可能回落 max）。
  const EXPECTED: Array<[string, string, string[], string?]> = [
    // 火山官方 Codex 接入文档四份一致：low/medium/high
    ["火山 Agent Plan", "ark-code-latest", ["low", "medium", "high"]],
    ["火山 Coding Plan", "ark-code-latest", ["low", "medium", "high"]],
    // 方舟深度思考文档：本模型无限制的通用四档（minimal=关思考直接回答）
    [
      "Volcengine Doubao",
      "doubao-seed-2-1-pro-260628",
      ["minimal", "low", "medium", "high"],
    ],
    // 混元官方枚举 low/high；hy3 开源 chat template 对其他值直接 raise
    ["Tencent Hunyuan", "hy3", ["low", "high"]],
    ["Tencent Hunyuan", "hy3-preview", ["low", "high"]],
    ["Tencent Hunyuan", "hy4-preview", ["none", "high"], "high"],
    // 腾讯 Token Plan（订阅线 /plan 端点）档位全部真 Key 实测（2026-08-31）：
    // glm-5.3 始终思考且档位严格枚举 low/high/max（medium/xhigh 直接 400，
    // 错误信息即枚举来源）；kimi-k2.7-code(-highspeed) 仅接受
    // thinking:enabled；minimax-m2.7 与国内 auto 关思考被静默忽略
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
    ["DeepSeek", "deepseek-flash", ["low", "high", "max"]],
    ["DeepSeek", "deepseek-v4-pro", ["low", "high", "max"]],
    // MiniMax-M3 官方 catalog=none/high；M3.1 Flash 思考不可关，实测 low–max 可用、none 报 2013；MiMo 2026-09-23 官方目录为四档、默认 low。
    ["MiniMax", "MiniMax-M3", ["none", "high"]],
    [
      "MiniMax",
      "MiniMax-M3.1-Flash",
      ["low", "medium", "high", "xhigh", "max"],
    ],
    ["MiniMax en", "MiniMax-M3", ["none", "high"]],
    [
      "MiniMax en",
      "MiniMax-M3.1-Flash",
      ["low", "medium", "high", "xhigh", "max"],
    ],
    ["Xiaomi MiMo", "mimo-v2.5-pro", ["none", "low", "medium", "high"], "low"],
    ["Xiaomi MiMo", "mimo-v2.5", ["none", "low", "medium", "high"], "low"],
    [
      "Xiaomi MiMo Token Plan (China)",
      "mimo-v2.5-pro",
      ["none", "low", "medium", "high"],
      "low",
    ],
    [
      "Xiaomi MiMo Token Plan (China)",
      "mimo-v2.5",
      ["none", "low", "medium", "high"],
      "low",
    ],
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
    // SiliconFlow .cn：平台仅 high/max，显式 high 与预设配置一致。
    ["SiliconFlow", "deepseek-ai/DeepSeek-V4-Flash", ["high", "max"], "high"],
    // 聚合平台尚未确认新 GLM 的开关/effort；单档不下发推理控制参数。
    ["AtlasCloud", "zai-org/glm-5.2", ["high"]],
    ["Novita AI", "zai-org/glm-5.3", ["high"]],
    // NIM K3 的官方枚举；显式 high 与预设配置一致。
    ["Nvidia", "moonshotai/kimi-k3", ["low", "high", "max"], "high"],
    // Groq gpt-oss（Chat 路由）：平台声明的 reasoning_effort 枚举
    // low/medium/high（console.groq.com/docs/reasoning，2026-10-14），含 high，
    // 无需显式默认
    ["Groq", "openai/gpt-oss-120b", ["low", "medium", "high"]],
    ["Groq", "openai/gpt-oss-20b", ["low", "medium", "high"]],
    // SambaNova（native Responses）：OpenAPI 声明 reasoning_effort 枚举
    // low/medium/high（无 none）；MiniMax-M3 的 effort 只是思考开关，只列 high
    ["SambaNova", "MiniMax-M3", ["high"]],
    ["SambaNova", "gpt-oss-120b", ["low", "medium", "high"]],
    // DigitalOcean（native Responses）：GPT-6.1 Sol 平台写 low 到 max，按 OpenAI
    // 官方列五档；GLM-5.3 平台声明 low/high/max；DeepSeek V4 Pro 0813 平台未
    // 声明，退回 DeepSeek 官方档位
    [
      "DigitalOcean",
      "openai-gpt-6-1-sol",
      ["low", "medium", "high", "xhigh", "max"],
    ],
    ["DigitalOcean", "glm-5.3", ["low", "high", "max"]],
    ["DigitalOcean", "deepseek-v4-pro-0813", ["low", "high", "max"]],
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
    // Kimi 开放平台（原生 Responses 直连，默认模型 kimi-k3）：k3 三档不声明
    // default——native 模板默认 high ∈ 子集故后端保留 high（= 预设
    // config.toml 的 model_reasoning_effort），官方默认 max 只是 API 侧未显式
    // 传 effort 时的行为；k2.7-code 始终思考且官方标注不支持 effort → 单档。
    // 均关不掉思考无 none
    ["Kimi", "kimi-k3", ["low", "high", "max"]],
    ["Kimi", "kimi-k2.7-code", ["high"]],
    // Kimi Code 端点（原生 Responses 直连）：k3/k3-256k 官方 models.json 明写
    // default_reasoning_level "high"，与 native 模板回落值相同——照抄官方目录
    // 的显式声明（表单可见性优先，MiniMax/MiMo 先例）故仍有第四位期望；
    // 标准 kimi-for-coding 已升级到 K2.8 Preview；highspeed 保留原来的单档目录。
    ["Kimi For Coding", "kimi-for-coding", ["low", "high", "max"], "high"],
    ["Kimi For Coding", "kimi-for-coding-highspeed", ["high"]],
    ["Kimi For Coding", "k3", ["low", "high", "max"], "high"],
    ["Kimi For Coding", "k3-256k", ["low", "high", "max"], "high"],
    // OpenCode Go（Zen 网关）：opencode 客户端 variants() 严格按各模型在
    // models.dev 的 reasoning_options 声明发 reasoning_effort（provider/
    // transform.ts）——GLM5.3 两款有 low/high/max，Kimi K3 仅 max。
    // 代理转换层按同一张表逐模型钳制；GLM 默认 high 与预设配置一致。
    ["OpenCode Go", "glm-5.3", ["low", "high", "max"], "high"],
    ["OpenCode Go", "glm-5.3-flash", ["low", "high", "max"], "high"],
    ["OpenCode Go", "kimi-k3", ["max"]],
    ["OpenCode Go", "deepseek-v4-pro", ["high", "max"]],
    ["OpenCode Go", "deepseek-v4-flash", ["low", "high", "max"]],
    // 千问官方 Codex 页只发布一份 model-catalog.local.json，且该元数据段落
    // 位于套餐分页之前（help.aliyun.com/zh/model-studio/codex，2026-09-08
    // 核对）：qwen3.8-max 档位 low/medium/xhigh、默认 xhigh（≠ 模板回落的
    // none/high，故显式声明）；按量付费与 Token Plan 同源同一份
    ["千问AI平台", "qwen3.8-max", ["low", "medium", "xhigh"], "xhigh"],
    ["千问AI平台", "qwen3.8-2.4t-a95b", ["low", "medium", "xhigh"], "xhigh"],
    ["千问AI平台", "qwen3.8-27b", ["low", "medium", "xhigh"], "xhigh"],
    ["QwenCloud", "qwen3.8-2.4t-a95b", ["low", "medium", "xhigh"], "xhigh"],
    ["QwenCloud", "qwen3.8-27b", ["low", "medium", "xhigh"], "xhigh"],
    // Ollama Cloud（原生 Responses 直连）：平台 /api/show 的 thinking.values
    //（2026-10-14）。关闭值是 false 而非 none，映射未证实故不列 none；每档
    // 都含 high，沿用模板默认 high，不声明 default
    ["Ollama Cloud", "glm-5.3", ["low", "high", "max"]],
    ["Ollama Cloud", "glm-5.3-flash", ["low", "high", "max"]],
    ["Ollama Cloud", "kimi-k3", ["low", "high", "max"]],
    ["Ollama Cloud", "deepseek-v4-pro:0813", ["low", "high", "max"]],
    ["Ollama Cloud", "deepseek-v4.1-flash", ["low", "high", "max"]],
    // Ollama 本机（原生 Responses 直连）：gpt-oss 取 ollama.com/api/show 的
    // thinking.values；qwen3.5 只支持开关，文档写 none 映射为关（2026-10-14）
    ["Ollama", "gpt-oss:20b", ["low", "medium", "high"]],
    ["Ollama", "gpt-oss:120b", ["low", "medium", "high"]],
    ["Ollama", "qwen3.5", ["none", "high"]],
    // LM Studio 本机（原生 Responses 直连）：取 LM Studio 声明的
    // reasoning.effort 枚举（lmstudio.ai/blog/lmstudio-v0.3.29，2026-10-14）
    ["LM Studio", "openai/gpt-oss-20b", ["low", "medium", "high"]],
    // llama.cpp 本机（原生 Responses 直连）：Qwen3-Coder 不思考，llama-server 把
    // reasoning_effort 交给模型模板（PR #26941，2026-10-14），该模板不读，照
    // Nebius 先例只钉单档 high
    [
      "llama.cpp",
      "ggml-org/Qwen3-Coder-30B-A3B-Instruct-Q8_0-GGUF",
      ["high"],
    ],
    // vLLM 本机（原生 Responses 直连）：vLLM 用户没设 enable_thinking 时按
    // effort != "none" 开关思考（responses/protocol.py，v0.31.0，2026-10-14），
    // Qwen3.6 模板只有开关，故两态
    ["vLLM", "Qwen/Qwen3.6-27B", ["none", "high"]],
    // Baseten（Chat 路由，reasoning_effort 透传）：docs.baseten.co/inference/
    // model-apis/reasoning（2026-10-14）。GLM-5.3 系 none 按 low 处理（假关）
    // 故不列 none；V4.1 Flash 不支持 low；Kimi K3 API 默认 max，四款都显式
    // high 与 config.toml 一致
    ["Baseten", "zai-org/GLM-5.3", ["low", "high", "max"], "high"],
    ["Baseten", "moonshotai/Kimi-K3", ["none", "low", "high", "max"], "high"],
    ["Baseten", "zai-org/GLM-5.3-Flash", ["low", "high", "max"], "high"],
    [
      "Baseten",
      "deepseek-ai/DeepSeek-V4.1-Flash",
      ["none", "high", "max"],
      "high",
    ],
    // Meta Model API（原生 Responses 直连）：dev.meta.ai/docs/models.md 与
    // reasoning.md（2026-10-14）。两款常开思考，none 返回 400 故不列；
    // minimal 到 max 全支持
    [
      "Meta Model API",
      "muse-wonder-1.0",
      ["minimal", "low", "medium", "high", "xhigh", "max"],
    ],
    [
      "Meta Model API",
      "muse-spark-1.3",
      ["minimal", "low", "medium", "high", "xhigh", "max"],
    ],
    // DeepInfra（原生 Responses 直连）：平台无逐模型档位声明
    //（docs.deepinfra.com/chat/reasoning.md，2026-10-14），退回厂商官方档位，
    // 同 DeepSeek / Kimi / Zhipu GLM 直连预设
    [
      "DeepInfra",
      "deepseek-ai/DeepSeek-V4-Flash-0731",
      ["low", "high", "max"],
    ],
    ["DeepInfra", "deepseek-ai/DeepSeek-V4-Pro-0813", ["low", "high", "max"]],
    ["DeepInfra", "moonshotai/Kimi-K3", ["low", "high", "max"]],
    ["DeepInfra", "zai-org/GLM-5.3", ["low", "high", "max"]],
    // Vercel AI Gateway（原生 Responses 直连）：GET ai-gateway.vercel.sh/codex/
    // v1/models 的 supported_reasoning_levels（2026-10-14）；GPT-6 Astra /
    // 6.1 Sol 另声明的 ultra 不放
    [
      "Vercel AI Gateway",
      "openai/gpt-6-astra",
      ["low", "medium", "high", "xhigh", "max"],
    ],
    [
      "Vercel AI Gateway",
      "openai/gpt-6.1-sol",
      ["low", "medium", "high", "xhigh", "max"],
    ],
    [
      "Vercel AI Gateway",
      "openai/gpt-6-luna",
      ["low", "medium", "high", "xhigh", "max"],
    ],
    [
      "Vercel AI Gateway",
      "anthropic/claude-sonnet-5.5",
      ["low", "medium", "high", "xhigh"],
    ],
    [
      "Vercel AI Gateway",
      "anthropic/claude-opus-5.5",
      ["low", "medium", "high", "xhigh"],
    ],
    // Nebius Token Factory（原生 Responses 直连）：平台不按模型声明档位，
    // Responses 的 reasoning.effort 枚举无 max（api.tokenfactory.nebius.com/
    // openapi.json，2026-10-14），实测前只钉单档 high
    ["Nebius Token Factory", "zai-org/GLM-5.3", ["high"]],
    ["Nebius Token Factory", "zai-org/GLM-5.3-Flash", ["high"]],
    ["Nebius Token Factory", "moonshotai/Kimi-K3", ["high"]],
    ["Nebius Token Factory", "moonshotai/Kimi-K2.7-Code", ["high"]],
    ["Nebius Token Factory", "deepseek-ai/DeepSeek-V4-Pro-0813", ["high"]],
    // Hugging Face（Chat 路由，不下发推理参数）：router.huggingface.co/v1/models
    // 不按模型声明思考开关与档位（2026-10-14），上游钉 :baseten，只钉单档 high
    ["Hugging Face", "zai-org/GLM-5.3:baseten", ["high"]],
    ["Hugging Face", "zai-org/GLM-5.3-Flash:baseten", ["high"]],
    ["Hugging Face", "moonshotai/Kimi-K3:baseten", ["high"]],
    ["Hugging Face", "deepseek-ai/DeepSeek-V4-Pro-0813:baseten", ["high"]],
    // Fireworks AI（原生 Responses 直连）：Kimi K3 / DeepSeek V4.1 Flash 取 Chat
    // API 参考逐模型说明（docs.fireworks.ai/api-reference/post-chatcompletions，
    // 2026-10-14），GLM 5.3 / 5.3 Flash 参考未写，取 Pi 内置目录与 z.ai 官方
    [
      "Fireworks AI",
      "accounts/fireworks/models/glm-5p3",
      ["low", "high", "max"],
    ],
    [
      "Fireworks AI",
      "accounts/fireworks/models/kimi-k3",
      ["none", "low", "high", "max"],
    ],
    [
      "Fireworks AI",
      "accounts/fireworks/models/deepseek-v4p1-flash",
      ["none", "low", "high", "max"],
    ],
    [
      "Fireworks AI",
      "accounts/fireworks/models/glm-5p3-flash",
      ["low", "high", "max"],
    ],
  ];

  it.each(EXPECTED)(
    "%s / %s declares the vendor-documented levels",
    (presetName, modelId, levels, expectedDefault) => {
      const model = catalogModel(presetName, modelId);
      expect(model.reasoningLevels).toEqual(levels);
      // 显式默认来源见各预设注释；未声明时仍保留后端的 fallback 行为。
      expect(model.defaultReasoningLevel).toBe(expectedDefault);
    },
  );

  it("keeps deliberately-unfilled presets unfilled", () => {
    // OpenCode Go 的 MiMo 无 effort 声明，与 opencode 客户端一致（代理侧
    // 无表不发 reasoning_effort）。ModelScope 是否透传思考字段未证实，不造
    // 两态假开关。
    const UNFILLED: Array<[string, string]> = [
      ["OpenCode Go", "mimo-v2.5-pro"],
      ["ModelScope", "ZhipuAI/GLM-5.2"],
      // StepFun 无后缀 3.5-flash：官方未暴露 effort，单一常开思考态
      ["StepFun", "step-3.5-flash"],
      ["StepFun en", "step-3.5-flash"],
      // 千帆 Token Plan：三模型均不在 thinking 官方清单（2026-05-27 版）且
      // 无任何官方接入示例下发思考字段——无证据不造档位
      ["Baidu Qianfan Token Plan", "deepseek-v4-flash-0731"],
      ["Baidu Qianfan Token Plan", "glm-5.2"],
      ["Baidu Qianfan Token Plan", "kimi-k2.6"],
      // DeepInfra GLM-5.2：平台未声明、各处口径不一，未实测前不造档位
      ["DeepInfra", "zai-org/GLM-5.2"],
      // Mistral AI：开推理后流式 content 为分块数组、转换层暂不解析，且
      // 现有 effortValueMode 发不出 "none"，第一版不下发推理参数、不填档位
      ["Mistral AI", "mistral-medium-3-5"],
      ["Mistral AI", "mistral-small-2603"],
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
