/**
 * Codex++ 供应商预设
 * 基于 cc-switch (MIT) 的 codexProviderPresets.ts，作者 Jason Young
 * https://github.com/farion1231/cc-switch
 *
 * 提供一键填充供应商配置的预设模板，包括 Base URL、协议、模型列表等。
 * 去掉了 cc-switch 原始的商业合作标记（isPartner、partnerPromotionKey）。
 */

export type PresetCategory = "official" | "aggregator" | "third_party" | "cn_official";

export type RelayProtocol = "responses" | "chatCompletions";

export interface ProviderPreset {
  id: string;
  name: string;
  websiteUrl?: string;
  apiKeyUrl?: string;
  category: PresetCategory;
  baseUrl: string;
  protocol: RelayProtocol;
  model: string;
  modelList?: string[];
}

/**
 * 预设列表。选择任一预设会自动填充：
 * - name     → 供应商名称
 * - baseUrl  → API 端点
 * - protocol → responses / chatCompletions（根据上游实际协议）
 * - model    → 默认模型名
 * - modelList → 可选模型清单（换行分隔）
 */
export const PRESETS: ProviderPreset[] = [
  // ── 官方 ──
  {
    id: "openai",
    name: "OpenAI Official",
    category: "official",
    baseUrl: "https://api.openai.com/v1",
    protocol: "responses",
    model: "gpt-5.5",
    websiteUrl: "https://chatgpt.com/codex",
  },

  // ── 中国官方 ──
  {
    id: "deepseek",
    name: "DeepSeek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    category: "cn_official",
    baseUrl: "https://api.deepseek.com",
    protocol: "chatCompletions",
    model: "deepseek-v4-flash",
    modelList: ["deepseek-v4-flash", "deepseek-v4-pro"],
  },
  {
    id: "zhipu-glm",
    name: "Zhipu GLM",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    category: "cn_official",
    baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4",
    protocol: "chatCompletions",
    model: "glm-5.1",
    modelList: ["glm-5.1"],
  },
  {
    id: "kimi",
    name: "Kimi",
    websiteUrl: "https://platform.moonshot.cn",
    apiKeyUrl: "https://platform.moonshot.cn/console/api-keys",
    category: "cn_official",
    baseUrl: "https://api.moonshot.cn/v1",
    protocol: "chatCompletions",
    model: "kimi-k2.6",
    modelList: ["kimi-k2.6"],
  },
  {
    id: "bailian",
    name: "Bailian (Qwen)",
    websiteUrl: "https://bailian.console.aliyun.com",
    apiKeyUrl: "https://bailian.console.aliyun.com/#/api-key",
    category: "cn_official",
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    protocol: "chatCompletions",
    model: "qwen3-coder-plus",
    modelList: ["qwen3-coder-plus", "qwen3-max"],
  },
  {
    id: "stepfun",
    name: "StepFun",
    websiteUrl: "https://platform.stepfun.com/step-plan",
    apiKeyUrl: "https://platform.stepfun.com/interface-key",
    category: "cn_official",
    baseUrl: "https://api.stepfun.com/step_plan/v1",
    protocol: "chatCompletions",
    model: "step-3.5-flash-2603",
    modelList: ["step-3.5-flash-2603", "step-3.5-flash"],
  },
  {
    id: "minimax",
    name: "MiniMax",
    websiteUrl: "https://platform.minimaxi.com",
    apiKeyUrl: "https://platform.minimaxi.com/subscribe/coding-plan",
    category: "cn_official",
    baseUrl: "https://api.minimaxi.com/v1",
    protocol: "chatCompletions",
    model: "MiniMax-M2.7",
    modelList: ["MiniMax-M2.7"],
  },
  {
    id: "volcano-ark",
    name: "火山引擎 Ark",
    websiteUrl: "https://www.volcengine.com/product/ark",
    apiKeyUrl: "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey",
    category: "cn_official",
    baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
    protocol: "chatCompletions",
    model: "ark-code-latest",
    modelList: ["ark-code-latest"],
  },
  {
    id: "baidu-qianfan",
    name: "百度千帆 Coding Plan",
    category: "cn_official",
    baseUrl: "https://qianfan.baidubce.com/v2/coding",
    protocol: "chatCompletions",
    model: "qianfan-code-latest",
    websiteUrl: "https://cloud.baidu.com/product/qianfan_modelbuilder",
  },
  {
    id: "xiaomi-mimo",
    name: "小米 MiMo",
    category: "cn_official",
    baseUrl: "https://api.xiaomimimo.com/v1",
    protocol: "chatCompletions",
    model: "mimo-v2.5-pro",
    modelList: ["mimo-v2.5-pro"],
    websiteUrl: "https://platform.xiaomimimo.com",
  },
  {
    id: "modelscope",
    name: "ModelScope",
    category: "cn_official",
    baseUrl: "https://api-inference.modelscope.cn/v1",
    protocol: "chatCompletions",
    model: "ZhipuAI/GLM-5.1",
    modelList: ["ZhipuAI/GLM-5.1"],
    websiteUrl: "https://modelscope.cn",
  },
  {
    id: "longcat",
    name: "Longcat",
    category: "cn_official",
    baseUrl: "https://api.longcat.chat/openai/v1",
    protocol: "chatCompletions",
    model: "LongCat-Flash-Chat",
    modelList: ["LongCat-Flash-Chat"],
    websiteUrl: "https://longcat.chat/platform",
  },

  // ── 聚合/中转 ──
  {
    id: "jojocode",
    name: "JOJO Code",
    websiteUrl: "https://jojocode.com/",
    apiKeyUrl: "https://jojocode.com/",
    category: "aggregator",
    baseUrl: "https://jojocode.com/v1",
    protocol: "responses",
    model: "gpt-5.5",
  },
  {
    id: "jojocode-max",
    name: "JOJO Code 包月",
    websiteUrl: "https://max.jojocode.com/",
    apiKeyUrl: "https://max.jojocode.com/",
    category: "aggregator",
    baseUrl: "https://max.jojocode.com/v1",
    protocol: "responses",
    model: "gpt-5.5",
  },
  {
    id: "runapi",
    name: "RunAPI",
    websiteUrl: "https://runapi.co",
    apiKeyUrl: "https://runapi.co",
    category: "aggregator",
    baseUrl: "https://runapi.co/v1",
    protocol: "responses",
    model: "gpt-5.5",
  },
  {
    id: "siliconflow",
    name: "SiliconFlow",
    websiteUrl: "https://siliconflow.cn",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/drGuwc9k",
    category: "aggregator",
    baseUrl: "https://api.siliconflow.cn/v1",
    protocol: "chatCompletions",
    model: "Pro/MiniMaxAI/MiniMax-M2.7",
    modelList: ["Pro/MiniMaxAI/MiniMax-M2.7"],
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    category: "aggregator",
    baseUrl: "https://openrouter.ai/api/v1",
    protocol: "chatCompletions",
    model: "gpt-5.5",
  },
  {
    id: "aihubmix",
    name: "AiHubMix",
    category: "aggregator",
    baseUrl: "https://aihubmix.com/v1",
    protocol: "responses",
    model: "gpt-5.5",
    websiteUrl: "https://aihubmix.com",
  },
  {
    id: "apikeyfun",
    name: "APIKEY.FUN",
    category: "aggregator",
    baseUrl: "https://api.apikey.fun/v1",
    protocol: "responses",
    model: "gpt-5.5",
    modelList: ["gpt-5.5"],
    websiteUrl: "https://apikey.fun",
  },
  {
    id: "pateway",
    name: "PatewayAI",
    category: "aggregator",
    baseUrl: "https://api.pateway.ai/v1",
    protocol: "responses",
    model: "gpt-5.5",
    websiteUrl: "https://pateway.ai",
  },
  {
    id: "therouter",
    name: "TheRouter",
    category: "aggregator",
    baseUrl: "https://api.therouter.ai/v1",
    protocol: "chatCompletions",
    model: "openai/gpt-5.3-codex",
    websiteUrl: "https://therouter.ai",
  },
  {
    id: "novita",
    name: "Novita AI",
    category: "aggregator",
    baseUrl: "https://api.novita.ai/openai/v1",
    protocol: "chatCompletions",
    model: "zai-org/glm-5.1",
    modelList: ["zai-org/glm-5.1"],
    websiteUrl: "https://novita.ai",
  },
  {
    id: "shengsuanyun",
    name: "Shengsuanyun",
    category: "aggregator",
    baseUrl: "https://router.shengsuanyun.com/api/v1",
    protocol: "chatCompletions",
    model: "openai/gpt-5.5",
    websiteUrl: "https://www.shengsuanyun.com",
  },
  {
    id: "ccsub",
    name: "CCSub",
    category: "aggregator",
    baseUrl: "https://www.ccsub.net/v1",
    protocol: "responses",
    model: "gpt-5.5",
    websiteUrl: "https://www.ccsub.net",
  },

  // ── 第三方 ──
  {
    id: "azure",
    name: "Azure OpenAI",
    category: "third_party",
    baseUrl: "https://YOUR_RESOURCE_NAME.openai.azure.com/openai",
    protocol: "responses",
    model: "gpt-5.5",
    websiteUrl: "https://learn.microsoft.com/en-us/azure/ai-foundry/openai/how-to/codex",
  },
];
