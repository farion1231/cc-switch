/**
 * Codex 预设供应商配置模板
 */
import { ProviderCategory } from "../types";
import type {
  CodexApiFormat,
  CodexCatalogModel,
  CodexChatReasoning,
  PromptCacheRoutingMode,
} from "../types";
import type { PresetTheme } from "./claudeProviderPresets";

export interface CodexProviderPreset {
  name: string;
  nameKey?: string; // i18n key for localized display name
  websiteUrl: string;
  // 第三方供应商可提供单独的获取 API Key 链接
  apiKeyUrl?: string;
  auth: Record<string, any>; // 将写入 ~/.codex/auth.json
  config: string; // 将写入 ~/.codex/config.toml（TOML 字符串）
  isOfficial?: boolean; // 标识是否为官方预设
  isPartner?: boolean; // 标识是否为商业合作伙伴
  primePartner?: boolean; // 置顶合作伙伴（顶级）：徽章显示为心形
  partnerPromotionKey?: string; // 合作伙伴促销信息的 i18n key
  category?: ProviderCategory; // 新增：分类
  isCustomTemplate?: boolean; // 标识是否为自定义模板
  // 新增：请求地址候选列表（用于地址管理/测速）
  endpointCandidates?: string[];
  // 新增：视觉主题配置
  theme?: PresetTheme;
  // 图标配置
  icon?: string; // 图标名称
  iconColor?: string; // 图标颜色
  // Codex API 格式
  apiFormat?: CodexApiFormat;
  // 仅用于区分预设来源；ChatGPT/Codex 与 xAI/Grok 的认证流程彼此独立。
  providerType?: "codex_oauth" | "xai_oauth";
  // OAuth 预设：隐藏 API Key 输入，保存前要求已登录托管账号
  requiresOAuth?: boolean;
  // Codex Chat 本地路由模式下的模型目录
  modelCatalog?: CodexCatalogModel[];
  // Codex Responses -> Chat Completions reasoning capability defaults
  codexChatReasoning?: CodexChatReasoning;
  // Session-based prompt-cache routing override for Chat Completions upstreams
  promptCacheRouting?: PromptCacheRoutingMode;
}

/**
 * 生成第三方供应商的 auth.json
 */
export function generateThirdPartyAuth(apiKey: string): Record<string, any> {
  return {
    OPENAI_API_KEY: apiKey || "",
  };
}

/**
 * 生成第三方供应商的 config.toml
 */
export function generateThirdPartyConfig(
  providerName: string,
  baseUrl: string,
  modelName = "gpt-5.6-sol",
  options?: {
    // 托管 OAuth 预设（requiresOAuth 卡）必须传 false：这类卡无静态 key，
    // requires_openai_auth = true 会被后端 keyless 安全闸拒绝切换
    // （provider.codex.config.official_auth_fallback）。
    requiresOpenAiAuth?: boolean;
  },
): string {
  const tomlString = (value: string) => JSON.stringify(value);
  const requiresOpenAiAuth = options?.requiresOpenAiAuth ?? true;

  return `model_provider = "custom"
model = ${tomlString(modelName)}
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = ${tomlString(providerName)}
base_url = ${tomlString(baseUrl)}
wire_api = "responses"
requires_openai_auth = ${requiresOpenAiAuth}`;
}

function modelCatalog(
  models: Array<
    | string
    | {
        model: string;
        displayName?: string;
        contextWindow?: number;
        // Native Responses (direct) overrides for the generated
        // model-catalogs.json. Omitted input modalities are inferred by the
        // backend: confirmed text-only models stay text-only; everything else
        // defaults to text+image.
        supportsParallelToolCalls?: boolean;
        inputModalities?: string[];
        // Vendor's OFFICIAL base_instructions; omit to inherit the neutral
        // template default. Required by Codex, so the backend always emits one.
        baseInstructions?: string;
        // Reasoning efforts the vendor's endpoint actually accepts (subset of
        // none/minimal/low/medium/high/xhigh/max/ultra). Omit to keep the
        // template's conservative none/high default. Pre-filled from official
        // vendor docs; users can still edit per provider in the form.
        reasoningLevels?: string[];
        defaultReasoningLevel?: string;
      }
  >,
): CodexCatalogModel[] {
  return models.map((entry) =>
    typeof entry === "string"
      ? { model: entry }
      : {
          model: entry.model,
          displayName: entry.displayName,
          contextWindow: entry.contextWindow,
          supportsParallelToolCalls: entry.supportsParallelToolCalls,
          inputModalities: entry.inputModalities,
          baseInstructions: entry.baseInstructions,
          reasoningLevels: entry.reasoningLevels,
          defaultReasoningLevel: entry.defaultReasoningLevel,
        },
  );
}

export const codexProviderPresets: CodexProviderPreset[] = [
  {
    name: "OpenAI Official",
    websiteUrl: "https://chatgpt.com/codex",
    isOfficial: true,
    category: "official",
    providerType: "codex_oauth",
    auth: {},
    config: ``,
    theme: {
      icon: "codex",
      backgroundColor: "#1F2937", // gray-800
      textColor: "#FFFFFF",
    },
    icon: "openai",
    iconColor: "#00A67E",
  },
  // ===== 赞助商预设：文件顺序 = 应用内展示顺序，与 README 赞助商表对齐 =====
  {
    name: "Kimi",
    primePartner: true,
    websiteUrl: "https://platform.kimi.com?aff=cc-switch",
    apiKeyUrl: "https://platform.kimi.com/console/api-keys?aff=cc-switch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "kimi",
      "https://api.moonshot.cn/v1",
      "kimi-k2.7-code",
    ),
    endpointCandidates: ["https://api.moonshot.cn/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      // 档位照抄官方参数文档（2026-08-15 盘点）：k2.7-code 始终思考、官方
      // 标注不支持 reasoning_effort → 单档 high（防模板假差异档，LongCat
      // 先例）；k3 不可关思考、顶层 reasoning_effort 三档官方默认 max——
      // 不声明 default：模板默认 medium ∉ 子集时后端回落最高档 = max，恰合
      // 官方默认。两模型都关不掉思考，none 一律不列
      {
        model: "kimi-k2.7-code",
        displayName: "Kimi K2.7 Code",
        contextWindow: 262144,
        reasoningLevels: ["high"],
      },
      {
        model: "kimi-k3",
        displayName: "Kimi K3",
        contextWindow: 1048576,
        reasoningLevels: ["low", "high", "max"],
      },
    ]),
    // supportsEffort:true（2026-08-15 盘点）：Kimi 官方 Codex 接入文档
    //（platform.kimi.com/docs/guide/codex-kimi.md，直接以 CC Switch 为例）
    // 要求「支持思考模式 开启 / 支持推理强度 开启」；k3 的 reasoning_effort
    // 是顶层字符串。effortValueMode 不声明=passthrough 原值透传（勿用
    // deepseek 模式，会把 low 压成 high）。注：官方参数页写 k3"不应传入
    // thinking"、与接入指南"思考模式开启"矛盾，现网无事故报告，按接入指南
    // 保持 thinking 注入；用户报 Kimi 400 时首查此处
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    partnerPromotionKey: "kimi",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "Kimi For Coding",
    primePartner: true,
    websiteUrl: "https://www.kimi.com/code/?aff=cc-switch",
    apiKeyUrl: "https://www.kimi.com/code/?aff=cc-switch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "kimi_coding",
      "https://api.kimi.com/coding/v1",
      "kimi-for-coding",
    ),
    endpointCandidates: ["https://api.kimi.com/coding/v1"],
    apiFormat: "openai_chat",
    promptCacheRouting: "enabled",
    modelCatalog: modelCatalog([
      // Kimi Code 官方模型表（2026-08-15 盘点）：kimi-for-coding(-highspeed)
      // =K2.7 Code、Thinking 恒 ON 无档位 → 单档 high；k3/k3-256k 三档官方
      // 默认 high（与开放平台的默认 max 不同，须显式 default 防后端回落到
      // 最高档 max）。none 不列——该网关关思考=静默路由到 K2.6（换模型换
      // 计费）。网关 effort 白名单 ultra/max/xhigh/high/medium/low/minimum/
      // light/none，未知值 400（Codex 的 minimal 不在内，档位子集已挡住
      // 选择器，用户自改档位需自担）
      {
        model: "kimi-for-coding",
        displayName: "Kimi For Coding",
        contextWindow: 262144,
        reasoningLevels: ["high"],
      },
      {
        model: "kimi-for-coding-highspeed",
        displayName: "Kimi For Coding HighSpeed",
        contextWindow: 262144,
        reasoningLevels: ["high"],
      },
      {
        model: "k3",
        displayName: "Kimi K3",
        contextWindow: 1048576,
        reasoningLevels: ["low", "high", "max"],
        defaultReasoningLevel: "high",
      },
      {
        model: "k3-256k",
        displayName: "Kimi K3 256K",
        contextWindow: 262144,
        reasoningLevels: ["low", "high", "max"],
        defaultReasoningLevel: "high",
      },
    ]),
    // 官方 Codex 接入文档（kimi.com/code/docs/third-party-tools/codex.html，
    // 以 CC Switch 为例）：「支持思考模式 开启（必须——关闭后 K3/K2.7 Code
    // 都会被路由到 K2.6）/ 支持思考等级 开启」。effortValueMode 不声明=
    // passthrough；网关自身对 effort 做归一映射（null→high、none→关思考）
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.ai",
    apiKeyUrl: "https://www.packyapi.ai/register?aff=cc-switch",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "packycode",
      "https://www.packyapi.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://www.packyapi.ai/v1",
      "https://cf.api.fan/v1",
      "https://slb-v1.api.fan/v1",
      "https://www.packyapi.com/v1",
    ],
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "packycode", // 促销信息 i18n key
    icon: "packycode",
  },
  {
    name: "ZetaAPI",
    websiteUrl: "https://zetaapi.ai",
    apiKeyUrl: "https://zetaapi.ai/go/u117",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "zetaapi",
      "https://api.zetaapi.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.zetaapi.ai/v1"],
    isPartner: true,
    partnerPromotionKey: "zetaapi",
    icon: "zetaapi",
  },
  {
    name: "APINebula",
    websiteUrl: "https://apinebula.ai",
    apiKeyUrl: "https://apinebula.ai/VjM74M",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
review_model = "gpt-5.6-sol"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "APINebula"
base_url = "https://apinebula.ai/v1"
wire_api = "responses"
requires_openai_auth = true`,
    endpointCandidates: ["https://apinebula.ai/v1"],
    apiFormat: "openai_responses",
    isPartner: true,
    partnerPromotionKey: "apinebula",
    icon: "apinebula",
  },
  {
    name: "AICodeMirror",
    websiteUrl: "https://www.aicodemirror.ai",
    apiKeyUrl: "https://www.aicodemirror.ai/register?invitecode=9915W3",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "aicodemirror",
      "https://api.aicodemirror.ai/api/codex/backend-api/codex",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://api.aicodemirror.ai/api/codex/backend-api/codex",
    ],
    isPartner: true,
    partnerPromotionKey: "aicodemirror",
    icon: "aicodemirror",
    iconColor: "#000000",
  },
  {
    name: "PatewayAI",
    websiteUrl: "https://pateway.ai",
    apiKeyUrl: "https://pateway.ai/?ch=etzpm8&aff=WB6M6F67#/",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "patewayai",
      "https://api.pateway.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.pateway.ai/v1"],
    isPartner: true,
    partnerPromotionKey: "patewayai",
    icon: "pateway",
  },
  {
    name: "FennoAI",
    websiteUrl: "https://api.fenno.ai",
    apiKeyUrl:
      "https://api.fenno.ai/register?redirect=/purchase?tab=subscription%26group=16&aff=P9MR3D3PLCNL",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "fenno",
      "https://api.fenno.ai",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.fenno.ai"],
    isPartner: true,
    partnerPromotionKey: "fenno",
    icon: "fenno",
  },
  {
    name: "RunAPI",
    websiteUrl: "https://runapi.host",
    apiKeyUrl: "https://runapi.host/register?aff=iOKB",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "runapi",
      "https://runapi.host/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://runapi.host/v1", "https://runapi.co/v1"],
    isPartner: true,
    partnerPromotionKey: "runapi",
    icon: "runapi",
  },
  {
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "shengsuanyun",
      "https://router.shengsuanyun.com/api/v1",
      "openai/gpt-5.6-sol",
    ),
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "shengsuanyun",
    icon: "shengsuanyun",
  },
  {
    name: "AIGoCode",
    websiteUrl: "https://aigocode.app",
    apiKeyUrl: "https://aigocode.app/invite/CC-SWITCH",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "aigocode",
      "https://api.aigocode.app",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.aigocode.app"],
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "aigocode", // 促销信息 i18n key
    icon: "aigocode",
    iconColor: "#5B7FFF",
  },
  {
    name: "Qiniu",
    nameKey: "providerForm.presets.qiniu",
    websiteUrl: "https://s.qiniu.com/nMvAvy",
    apiKeyUrl: "https://s.qiniu.com/nMvAvy",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "qiniu",
      "https://api.qnaigc.com/bypass/openai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://api.qnaigc.com/bypass/openai/v1",
      "https://api.modelink.ai/bypass/openai/v1",
    ],
    isPartner: true,
    partnerPromotionKey: "qiniu",
    icon: "qiniu",
  },
  {
    name: "AICoding",
    websiteUrl: "https://aicoding.inc",
    apiKeyUrl: "https://aicoding.inc/i/CCSWITCH",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "aicoding",
      "https://api.aicoding.inc",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.aicoding.inc"],
    isPartner: true,
    partnerPromotionKey: "aicoding",
    icon: "aicoding",
    iconColor: "#000000",
  },
  {
    name: "SubRouter",
    websiteUrl: "https://subrouter.ai",
    apiKeyUrl: "https://subrouter.ai/register?aff=l3ri",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "subrouter",
      "https://subrouter.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://subrouter.ai/v1"],
    isPartner: true,
    partnerPromotionKey: "subrouter",
    icon: "subrouter",
  },
  {
    name: "APIKEY.FUN",
    websiteUrl: "https://apikey.fun",
    apiKeyUrl: "https://apikey.fun/register?aff=CCSwitch",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
review_model = "gpt-5.6-sol"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "APIKEY.FUN"
base_url = "https://api.apikey.fun/v1"
wire_api = "responses"
requires_openai_auth = true`,
    endpointCandidates: [
      "https://api.apikey.fun/v1",
      "https://slb.apikey.fun/v1",
    ],
    apiFormat: "openai_responses",
    isPartner: true,
    partnerPromotionKey: "apikeyfun",
    icon: "apikeyfun",
  },
  {
    name: "9527CODE",
    websiteUrl: "https://9527.codes",
    apiKeyUrl: "https://9527.codes/register?aff=e5zI",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "9527code",
      "https://9527.codes/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://9527.codes/v1",
      "https://api.9527.codes/v1",
      "https://cdn.9527.codes/v1",
    ],
    isPartner: true,
    partnerPromotionKey: "9527code",
    icon: "9527code",
  },
  {
    name: "Code0",
    websiteUrl: "https://code0.ai",
    apiKeyUrl: "https://code0.ai/agent/register/B2XHxGjGmRvqgznY",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "code0",
      "https://code0.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://code0.ai/v1"],
    isPartner: true,
    partnerPromotionKey: "code0",
    icon: "code0",
  },
  {
    name: "TeamoRouter",
    websiteUrl: "https://teamorouter.cn",
    apiKeyUrl:
      "https://teamorouter.cn/?utm_source=cc_switch&utm_medium=referral&utm_campaign=ai_directory",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "teamorouter",
      "https://api.teamorouter.cn/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://api.teamorouter.cn/v1",
      "https://api.teamorouter.com/v1",
    ],
    isPartner: true,
    partnerPromotionKey: "teamorouter",
    icon: "teamorouter",
  },
  {
    name: "PPIO",
    websiteUrl: "https://ppio.com",
    apiKeyUrl: "https://ppio.com/activity/ccswitch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "ppio",
      "https://api.ppio.com/openai/v1",
      "deepseek/deepseek-v4-flash-0731",
    ),
    endpointCandidates: ["https://api.ppio.com/openai/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "deepseek/deepseek-v4-flash-0731",
        displayName: "Deepseek V4 Flash 0731",
        contextWindow: 1048576,
        inputModalities: ["text"],
      },
    ]),
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: false,
      thinkingParam: "thinking",
      effortParam: "none",
      outputFormat: "reasoning_content",
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ppio",
    icon: "ppio",
    iconColor: "#2874FF",
  },
  {
    name: "ClaudeCN",
    websiteUrl: "https://claudecn.top",
    apiKeyUrl: "https://claudecn.ai/register?aff=HEL9",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "claudecn",
      "https://claudecn.top/v1",
      "gpt-5.6-sol",
    ),
    isPartner: true,
    partnerPromotionKey: "claudecn",
    icon: "claudecn",
  },
  {
    name: "火山 Agent Plan",
    websiteUrl:
      "https://www.volcengine.com/activity/agentplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_source=OWO&utm_medium=devrel-1&utm_campaign=hw&utm_term=ccswitch&utm_content=hw",
    apiKeyUrl:
      "https://www.volcengine.com/activity/agentplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_source=OWO&utm_medium=devrel-1&utm_campaign=hw&utm_term=ccswitch&utm_content=hw",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "ark_agentplan",
      "https://ark.cn-beijing.volces.com/api/plan/v3",
      "ark-code-latest",
    ),
    // ⚠️ 计费红线（官方 warning）：Agent Plan 必须走 /api/plan/v3；
    // 按量端点 /api/v3 不消耗套餐额度、按量另计费，Coding Plan 的
    // /api/coding/v3 是另一份订阅——两者都绝不能混入候选
    endpointCandidates: ["https://ark.cn-beijing.volces.com/api/plan/v3"],
    // 官方 Codex 文档（volcengine.com/docs/82379/2556056，2026-07 更新）：
    // Agent Plan /api/plan/v3 与 Coding Plan /api/coding/v3 均已支持
    // Responses API（wire_api=responses），无需路由接管转换
    apiFormat: "openai_responses",
    modelCatalog: modelCatalog([
      {
        model: "ark-code-latest",
        displayName: "Ark Code Latest",
        contextWindow: 256000,
        // 四份官方 Codex 接入文档（82379/2556054~2556057）一致限定
        // model_reasoning_effort 只能是 low/medium/high；none/xhigh/max 是
        // glm-5-2 专属值（82379/1449737），别名可指向任意后端模型故不可填
        reasoningLevels: ["low", "medium", "high"],
      },
    ]),
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "volcengine_agentplan",
    icon: "huoshan",
    iconColor: "#3370FF",
  },
  {
    name: "火山 Coding Plan",
    websiteUrl:
      "https://www.volcengine.com/activity/codingplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://www.volcengine.com/activity/codingplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "ark_codingplan",
      "https://ark.cn-beijing.volces.com/api/coding/v3",
      "ark-code-latest",
    ),
    // ⚠️ 计费红线（官方 warning）：Coding Plan 必须走 /api/coding/v3；
    // 按量端点 /api/v3 不消耗套餐额度、按量另计费，Agent Plan 的
    // /api/plan/v3 是另一份订阅——两者都绝不能混入候选
    endpointCandidates: ["https://ark.cn-beijing.volces.com/api/coding/v3"],
    // 官方 Codex 文档（volcengine.com/docs/82379/2556056，2026-07 更新）：
    // Coding Plan /api/coding/v3 已支持 Responses API（wire_api=responses），
    // 无需路由接管转换
    apiFormat: "openai_responses",
    modelCatalog: modelCatalog([
      {
        model: "ark-code-latest",
        displayName: "Ark Code Latest",
        contextWindow: 256000,
        // 同 Agent Plan：官方 Codex 文档钉死 low/medium/high 三档
        reasoningLevels: ["low", "medium", "high"],
      },
    ]),
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "volcengine_codingplan",
    icon: "huoshan",
    iconColor: "#3370FF",
  },
  {
    name: "BytePlus",
    websiteUrl:
      "https://www.byteplus.com/en/product/modelark?utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://www.byteplus.com/en/product/modelark?utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "byteplus",
      "https://ark.ap-southeast.bytepluses.com/api/coding/v3",
      "ark-code-latest",
    ),
    endpointCandidates: [
      "https://ark.ap-southeast.bytepluses.com/api/coding/v3",
    ],
    // 国际站已核实（2026-08-15 盘点）：BytePlus 官方 Codex 接入文档
    //（docs.byteplus.com/en/docs/ModelArk/2556056）标准 config.toml 的
    // base_url 就是本端点且 wire_api="responses"，OpenCode 文档亦明写
    // Responses 优先——与国内站火山双 Plan 对齐切原生直连
    apiFormat: "openai_responses",
    modelCatalog: modelCatalog([
      {
        model: "ark-code-latest",
        displayName: "Ark Code Latest",
        contextWindow: 256000,
        // 官方 Codex 文档 model_reasoning_effort 限定 low/medium/high，与
        // 国内站同名模型四份文档交叉印证。⚠️auto 路由别名固有不确定性：
        // 路由到 glm-5-2-260617 时官方明载 low/medium 按 high 等价处理
        reasoningLevels: ["low", "medium", "high"],
      },
    ]),
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "byteplus",
    icon: "byteplus",
    iconColor: "#3370FF",
  },
  {
    name: "DouBaoSeed",
    websiteUrl:
      "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey?apikey=%7B%7D&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey?apikey=%7B%7D&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "doubaoseed",
      "https://ark.cn-beijing.volces.com/api/v3",
      "doubao-seed-2-1-pro-260628",
    ),
    endpointCandidates: ["https://ark.cn-beijing.volces.com/api/v3"],
    // 火山方舟主数据面 /api/v3 原生支持 Responses API（/api/v3/responses），无需路由接管转换
    apiFormat: "openai_responses",
    // 无官方 catalog：合成 MiMo 式（shell_command 编辑、不发 freeform apply_patch），
    // 让 Codex 直连显示模型并避免 custom 工具被网关拒绝
    modelCatalog: modelCatalog([
      {
        model: "doubao-seed-2-1-pro-260628",
        displayName: "Doubao Seed 2.1 Pro",
        contextWindow: 262144,
        // 方舟深度思考文档（82379/1449737）7 值枚举中本模型无限制的通用四档；
        // none/xhigh 仅 glm-5-2、max 的 deepseek 名单标注 Responses 待支持。
        // minimal=方舟的"关闭思考直接回答"档；官方点名本模型服务端默认 high
        reasoningLevels: ["minimal", "low", "medium", "high"],
      },
    ]),
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "doubaoseed",
    icon: "doubao",
    iconColor: "#3370FF",
  },
  {
    name: "SiliconFlow",
    websiteUrl: "https://siliconflow.cn",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/YflgU2Ve",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "siliconflow",
      "https://api.siliconflow.cn/v1",
      "Pro/MiniMaxAI/MiniMax-M2.5",
    ),
    endpointCandidates: ["https://api.siliconflow.cn/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      // 2026-08-15 盘点：M2.7 在 SiliconFlow 从未上架（两站目录+404 三重佐证），
      // .cn 站换 M2.5（目录 JSON contextLen=196608）；档位不填——enable_thinking
      // 能否真正关闭 M2.5 无官方明文（模型卡 Playground schema 不构成证据）
      {
        model: "Pro/MiniMaxAI/MiniMax-M2.5",
        displayName: "Pro / MiniMax M2.5",
        contextWindow: 196608,
      },
    ]),
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "siliconflow",
    icon: "siliconflow",
    iconColor: "#6E29F6",
  },
  {
    name: "SiliconFlow en",
    websiteUrl: "https://siliconflow.com",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/YflgU2Ve",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "siliconflow_en",
      "https://api.siliconflow.com/v1",
      "MiniMaxAI/MiniMax-M3",
    ),
    endpointCandidates: ["https://api.siliconflow.com/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      // 2026-08-15 盘点：M2.7 在 SiliconFlow 从未上架，.com 站换 M3（官方页
      // 1M 窗口=1049K tokens；.cn 站无 M3 勿互套）。SiliconFlow 平台开关是
      // enable_thinking 布尔（后端按平台推断兜底），M3 可关思考 → 两态
      {
        model: "MiniMaxAI/MiniMax-M3",
        displayName: "MiniMax M3",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
    ]),
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "siliconflow",
    icon: "siliconflow",
    iconColor: "#000000",
  },
  {
    name: "A6API",
    websiteUrl: "https://www.a6api.com",
    apiKeyUrl: "https://a6api.com/register?aff=AqNr",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "a6api",
      "https://api.a6api.com/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.a6api.com/v1"],
    isPartner: true,
    partnerPromotionKey: "a6api",
    icon: "a6api",
  },
  {
    name: "AtlasCloud",
    websiteUrl: "https://www.atlascloud.ai/console/coding-plan",
    apiKeyUrl: "https://www.atlascloud.ai/console/coding-plan",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "zai-org/glm-5.1"
disable_response_storage = true

[model_providers.custom]
name = "AtlasCloud"
base_url = "https://api.atlascloud.ai/v1"
wire_api = "responses"
requires_openai_auth = true`,
    endpointCandidates: ["https://api.atlascloud.ai/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "zai-org/glm-5.1",
        displayName: "GLM 5.1",
        contextWindow: 200000,
      },
    ]),
    isPartner: true,
    partnerPromotionKey: "atlascloud",
    icon: "atlascloud",
  },
  {
    name: "Compshare",
    nameKey: "providerForm.presets.ucloud",
    websiteUrl: "https://www.compshare.cn",
    apiKeyUrl:
      "https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "compshare",
      "https://api.modelverse.cn/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.modelverse.cn/v1"],
    category: "aggregator",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "ucloud", // 促销信息 i18n key
    icon: "ucloud",
    iconColor: "#000000",
  },
  {
    name: "Compshare Coding Plan",
    nameKey: "providerForm.presets.ucloudCoding",
    websiteUrl: "https://www.compshare.cn",
    apiKeyUrl:
      "https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "compshare_coding",
      "https://cp.compshare.cn/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://cp.compshare.cn/v1"],
    category: "aggregator",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "ucloud", // 促销信息 i18n key（复用）
    icon: "ucloud",
    iconColor: "#000000",
  },
  {
    name: "CCSub",
    websiteUrl: "https://www.ccsub.net",
    apiKeyUrl: "https://www.ccsub.net/register?ref=Y6Z8DXEA",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "ccsub",
      "https://www.ccsub.net/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://www.ccsub.net/v1"],
    isPartner: true,
    partnerPromotionKey: "ccsub",
    icon: "ccsub",
  },
  {
    name: "SSSAiCode",
    websiteUrl: "https://sssaicodeapi.com",
    apiKeyUrl: "https://sssaicodeapi.com/register?ref=DCP0SM",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "sssaicode",
      "https://node-hk.sssaicodeapi.com/api/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://node-hk.sssaicodeapi.com/api/v1",
      "https://node-hk.sssaiapi.com/api/v1",
      "https://node-cf.sssaicodeapi.com/api/v1",
    ],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "sssaicode", // 促销信息 i18n key
    icon: "sssaicode",
    iconColor: "#000000",
  },
  {
    name: "Micu",
    websiteUrl: "https://www.micuapi.ai",
    apiKeyUrl: "https://www.micuapi.ai/register?aff=aOYQ",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "micu",
      "https://www.micuapi.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://www.micuapi.ai/v1"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "micu", // 促销信息 i18n key
    icon: "micu",
    iconColor: "#000000",
  },
  {
    name: "RightCode",
    websiteUrl: "https://www.rightapi.ai",
    apiKeyUrl: "https://www.rightapi.ai/register?aff=CCSWITCH",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "rightcode",
      "https://www.rightapi.ai/codex/v1",
      "gpt-5.6-sol",
    ),
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "rightcode",
    icon: "rc",
    iconColor: "#E96B2C",
  },
  {
    name: "ETok.ai",
    websiteUrl: "https://etok.ai",
    apiKeyUrl: "https://etok.ai",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "etok",
      "https://api.etok.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.etok.ai/v1"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "etok", // 促销信息 i18n key
    icon: "etok",
    iconColor: "#000000",
  },
  {
    name: "Cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "cubence",
      "https://api.cubence.com/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://api.cubence.com/v1",
      "https://api-cf.cubence.com/v1",
      "https://api-dmit.cubence.com/v1",
      "https://api-bwg.cubence.com/v1",
    ],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "cubence", // 促销信息 i18n key
    icon: "cubence",
    iconColor: "#000000",
  },
  {
    name: "CrazyRouter",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "crazyrouter",
      "https://cn.crazyrouter.com/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://cn.crazyrouter.com/v1"],
    isPartner: true,
    partnerPromotionKey: "crazyrouter",
    icon: "crazyrouter",
    iconColor: "#000000",
  },
  {
    name: "DMXAPI",
    websiteUrl: "https://www.dmxapi.cn",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "dmxapi",
      "https://www.dmxapi.cn/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://www.dmxapi.cn/v1"],
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "dmxapi", // 促销信息 i18n key
  },
  {
    name: "SudoCode.chat",
    websiteUrl: "https://sudocode.chat",
    apiKeyUrl:
      "https://sudocode.chat/sign-up?aff=CC-SWITCH&utm_source=cc-switch&utm_medium=sponsor&utm_campaign=ccswitch",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
review_model = "gpt-5.6-sol"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "SudoCode"
base_url = "https://api.sudocode.chat/v1"
wire_api = "responses"
requires_openai_auth = true`,
    endpointCandidates: ["https://api.sudocode.chat/v1"],
    apiFormat: "openai_responses",
    isPartner: true,
    partnerPromotionKey: "sudocode",
    icon: "sudocode",
  },
  {
    name: "SudoCode.us",
    websiteUrl: "https://sudocode.us",
    apiKeyUrl: "https://sudocode.us",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
review_model = "gpt-5.6-sol"
model_reasoning_effort = "high"
disable_response_storage = true
model_verbosity = "high"

[model_providers.custom]
name = "sudocode"
base_url = "https://sudocode.us/v1"
wire_api = "responses"
requires_openai_auth = true`,
    endpointCandidates: ["https://sudocode.us/v1", "https://sudocode.run/v1"],
    apiFormat: "openai_responses",
    isPartner: true,
    icon: "sudocode-us",
  },
  {
    name: "XycAi",
    websiteUrl: "https://xycai.us",
    apiKeyUrl: "https://xycai.us/register?aff=Uhu9",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "xycai",
      "https://apicdn.xycai.us/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://apicdn.xycai.us/v1",
      "https://apicdn.xyc.ai/v1",
    ],
    isPartner: true,
    partnerPromotionKey: "xycai",
    icon: "xycai",
  },
  // ===== 非赞助商预设：应用内展示按显示名排序，此处文件顺序不影响展示 =====
  {
    name: "Amux",
    websiteUrl: "https://amux.ai",
    apiKeyUrl: "https://amux.ai",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "amux",
      "https://api.amux.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.amux.ai/v1"],
    icon: "amux",
  },
  {
    name: "Azure OpenAI",
    websiteUrl:
      "https://learn.microsoft.com/en-us/azure/ai-foundry/openai/how-to/codex",
    category: "third_party",
    isOfficial: true,
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "Azure OpenAI"
base_url = "https://YOUR_RESOURCE_NAME.openai.azure.com/openai"
env_key = "OPENAI_API_KEY"
query_params = { "api-version" = "2025-04-01-preview" }
wire_api = "responses"
requires_openai_auth = true`,
    endpointCandidates: ["https://YOUR_RESOURCE_NAME.openai.azure.com/openai"],
    theme: {
      icon: "codex",
      backgroundColor: "#0078D4",
      textColor: "#FFFFFF",
    },
    icon: "azure",
    iconColor: "#0078D4",
  },
  {
    name: "DeepSeek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "deepseek",
      "https://api.deepseek.com",
      "deepseek-v4-flash",
    ),
    endpointCandidates: ["https://api.deepseek.com"],
    // DeepSeek 官方 Codex 文档（api-docs.deepseek.com → agent_integrations/codex）：
    // deepseek-v4-flash 原生 Responses（wire_api=responses 对自家 base_url），无需路由接管转换。
    // 后端按 deepseek.com host 直接镜像官方 models.json（freeform apply_patch +
    // GPT-5 harness + low/high/max 思考档，需 codex >= 0.144.0），这里只保留行清单与展示名。
    // 档位照抄官方 catalog（low/high/max 默认 high，2026-08-15 复核 flash/pro
    // 逐字节一致）：per-row 值会覆盖官方镜像，DeepSeek 官方目录变更时须同步这里
    // （Jason 2026-08-15 拍板：表单可见性优先于快照过时风险，"未设置"误导性更大）
    apiFormat: "openai_responses",
    modelCatalog: modelCatalog([
      {
        model: "deepseek-v4-flash",
        displayName: "DeepSeek V4 Flash",
        contextWindow: 1048576,
        reasoningLevels: ["low", "high", "max"],
      },
      // pro 已于 2026-08 开通 Responses/Codex 集成（官方 catalog 条目与 flash 仅差 priority）
      {
        model: "deepseek-v4-pro",
        displayName: "DeepSeek V4 Pro",
        contextWindow: 1048576,
        reasoningLevels: ["low", "high", "max"],
      },
    ]),
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#1E88E5",
  },
  {
    name: "Zhipu GLM",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    auth: generateThirdPartyAuth(""),
    // 智谱三端点分立（docs.bigmodel.cn/cn/coding-plan/tool/others）：Anthropic
    // /api/anthropic、OpenAI Chat /api/coding/paas/v4、OpenAI Responses /api/v1，
    // 并明示「错误配置端点将导致无法使用 GLM Coding Plan 套餐额度」。Codex 直连
    // 发的是 Responses wire，base_url 必须是 /api/v1；Chat 端点上的 /responses
    // 是严格旧网关（拒 type=custom 工具 → #6944 的 400）
    config: generateThirdPartyConfig(
      "zhipu_glm",
      "https://open.bigmodel.cn/api/v1",
      "glm-5.3",
    ),
    endpointCandidates: ["https://open.bigmodel.cn/api/v1"],
    // 官方 Codex 接入页（docs.bigmodel.cn/cn/coding-plan/tool/codex，2026-09-04
    // 核对）：wire_api=responses 对自家 /api/v1，与 MiMo/MiniMax 同为原生直连
    // → NativeResponses profile（shell_command 编辑、不发 freeform apply_patch；
    // 官方目录虽声明 freeform，无真机验证前按保守口径，不引入 400 风险）
    apiFormat: "openai_responses",
    // 档位/上下文/模态照抄官方 models.json：glm-5.3 low/high/max 默认 max；
    // glm-5-turbo 官方档位为空、默认 max——cc-switch 表达不了空档位（回落会得到
    // 模板 none/high，none 在原生直连下没有转换层兜底、会原样发给严格网关），
    // 按官方默认收成单档 max。两模型 input_modalities=["text"]、并行工具调用 true
    modelCatalog: modelCatalog([
      {
        model: "glm-5.3",
        displayName: "GLM-5.3",
        contextWindow: 1048576,
        inputModalities: ["text"],
        supportsParallelToolCalls: true,
        reasoningLevels: ["low", "high", "max"],
        defaultReasoningLevel: "max",
      },
      {
        model: "glm-5-turbo",
        displayName: "GLM-5-Turbo",
        contextWindow: 204800,
        inputModalities: ["text"],
        supportsParallelToolCalls: true,
        reasoningLevels: ["max"],
      },
    ]),
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
  },
  {
    name: "Zhipu GLM en",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe?ic=8JVLJQFSKB",
    auth: generateThirdPartyAuth(""),
    // 国际站同上（docs.z.ai/devpack/tool/others + devpack/tool/codex，2026-09-04
    // 核对）：Responses 端点 /api/v1，官方 models.json 仅列 glm-5.3
    config: generateThirdPartyConfig(
      "zhipu_glm_en",
      "https://api.z.ai/api/v1",
      "glm-5.3",
    ),
    endpointCandidates: ["https://api.z.ai/api/v1"],
    apiFormat: "openai_responses",
    modelCatalog: modelCatalog([
      {
        model: "glm-5.3",
        displayName: "GLM-5.3",
        contextWindow: 1048576,
        inputModalities: ["text"],
        supportsParallelToolCalls: true,
        reasoningLevels: ["low", "high", "max"],
        defaultReasoningLevel: "max",
      },
    ]),
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
  },
  {
    name: "Baidu Qianfan Coding Plan",
    websiteUrl: "https://cloud.baidu.com/product/qianfan_modelbuilder",
    apiKeyUrl:
      "https://console.bce.baidu.com/qianfan/ais/console/applicationConsole/application",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "qianfan_coding",
      "https://qianfan.baidubce.com/v2/coding",
      "qianfan-code-latest",
    ),
    endpointCandidates: ["https://qianfan.baidubce.com/v2/coding"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      // 两态（2026-08-15 盘点）：千帆 v2 官方 thinking:{type:enabled/disabled}
      // 覆盖 Coding Plan 主力六模型，官方 OpenCode 接入文档在 /v2/coding 上
      // 对 minimax-m2.5/glm-5/kimi-k2.5 照发该字段。⚠️别名固有缺陷：控制台把
      // qianfan-code-latest 解析到 ernie-4.5-turbo 时 none 不会真关思考
      {
        model: "qianfan-code-latest",
        displayName: "Qianfan Code Latest",
        contextWindow: 131072,
        reasoningLevels: ["none", "high"],
      },
    ]),
    // 千帆 v2 Chat API 官方顶层参数（与智谱同形态）；平台对不支持的参数
    // "忽略不报错"（官方多处明载），别名解析到非清单模型时只失效不 400
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: false,
      thinkingParam: "thinking",
      effortParam: "none",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "baidu",
    iconColor: "#2932E1",
  },
  {
    // Token Plan 个人版：2026-07-13 起替代 Coding Plan 发售（Coding Plan
    // 停止新购、存量可用至到期，故上面的旧预设保留）。无别名机制，直接
    // 指定真实模型 id；官方 Codex 接入指南 wire_api 省略=chat 默认，与
    // Coding Plan 同走本地路由。API Key 是订阅页专属 Key（非通用应用 Key）
    name: "Baidu Qianfan Token Plan",
    websiteUrl: "https://cloud.baidu.com/product/codingplan.html",
    apiKeyUrl: "https://console.bce.baidu.com/qianfan/resource/token-plan",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "qianfan_tokenplan",
      "https://qianfan.baidubce.com/v2/tokenplan/personal",
      "deepseek-v4-pro",
    ),
    endpointCandidates: ["https://qianfan.baidubce.com/v2/tokenplan/personal"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      // 阵容与排序=Token Plan 个人版文档（2026-08-14 版）；ernie-5.1 官方
      // 标注 8/20 下线不收。窗口=千帆平台模型列表页口径（2026-08-06 版，
      // glm-5.1 与官方 OpenCode 接入页 198000 双重印证）
      {
        model: "deepseek-v4-pro",
        displayName: "DeepSeek V4 Pro",
        contextWindow: 1048576,
        // thinking + reasoning_effort 双官方清单模型：none=关思考，high/max
        // =官方仅有的两档真实深度。不声明 default：官方对复杂 Agent 类请求
        // 自动置 max=回落结果，显式钉 high 反而会压低平台该行为
        reasoningLevels: ["none", "high", "max"],
      },
      {
        model: "deepseek-v4-flash",
        displayName: "DeepSeek V4 Flash",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high", "max"],
      },
      {
        // 平台模型列表无独立条目、思考双清单均未收录——窗口按 v4-flash
        // 同款填，档位无证据不造
        model: "deepseek-v4-flash-0731",
        displayName: "DeepSeek V4 Flash 0731",
        contextWindow: 1048576,
      },
      {
        // 千帆平台标 1M（≠智谱自家 coding 端点 200K 口径，窗口是平台部署
        // 属性）；thinking 清单（2026-05-27 版）未收录，档位不填
        model: "glm-5.2",
        displayName: "GLM-5.2",
        contextWindow: 1048576,
      },
      {
        model: "glm-5.1",
        displayName: "GLM-5.1",
        contextWindow: 198000,
        // thinking 清单内，且官方 OpenCode 接入页在 Token Plan 端点上对它
        // 一手下发 thinking:{type:"enabled"} → 真实两态
        reasoningLevels: ["none", "high"],
      },
      {
        // thinking 清单未收录，档位不填
        model: "kimi-k2.6",
        displayName: "Kimi K2.6",
        contextWindow: 262144,
      },
    ]),
    // 与 Coding Plan 的差异：这里开 supportsEffort——Coding Plan 因别名不知
    // 解析到谁而保持 false；Token Plan catalog 全为显式模型，默认模型
    // deepseek-v4-pro 在 reasoning_effort 官方清单内（清单仅 v4-pro/v4-flash，
    // 档位仅 high/max）。effortValueMode:"deepseek"（max/xhigh/ultra→max、
    // 其余→high）与千帆官方向下兼容映射（low/medium→high、xhigh→max）逐字
    // 吻合；非清单模型收到 reasoning_effort 按平台明文"忽略不报错"，无害
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      effortValueMode: "deepseek",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "baidu",
    iconColor: "#2932E1",
  },
  {
    name: "Bailian",
    websiteUrl: "https://bailian.console.aliyun.com",
    apiKeyUrl: "https://bailian.console.aliyun.com/#/api-key",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "bailian",
      "https://dashscope.aliyuncs.com/compatible-mode/v1",
      "qwen3-coder-plus",
    ),
    endpointCandidates: ["https://dashscope.aliyuncs.com/compatible-mode/v1"],
    // 阿里百炼 DashScope 原生支持 OpenAI Responses API（/compatible-mode/v1/responses，同一 base_url），无需路由接管转换
    apiFormat: "openai_responses",
    // 无官方 catalog：合成 MiMo 式（shell_command 编辑、不发 freeform apply_patch）
    modelCatalog: modelCatalog([
      {
        model: "qwen3-coder-plus",
        displayName: "Qwen3 Coder Plus",
        contextWindow: 1048576,
      },
    ]),
    category: "cn_official",
    icon: "bailian",
    iconColor: "#624AFF",
  },
  // ===== QwenCloud（DashScope 国际站）=====
  // 三条线的 base_url 与密钥互不通用，且协议档位不同：
  // 按量付费与 Token Plan 走 /compatible-mode/v1 原生 Responses；
  // Coding Plan 的地址是 /v1（没有 compatible-mode 段），官方明示只支持
  // Chat Completions，故单独标 openai_chat 让后端改写 wire_api。
  {
    name: "QwenCloud",
    websiteUrl: "https://www.qwencloud.com",
    apiKeyUrl: "https://home.qwencloud.com/api-keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "qwencloud",
      "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
      "qwen3.7-max",
    ),
    endpointCandidates: [
      "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
    ],
    apiFormat: "openai_responses",
    modelCatalog: modelCatalog([
      {
        model: "qwen3.7-max",
        displayName: "Qwen3.7 Max",
        contextWindow: 1000000,
        inputModalities: ["text"],
      },
      {
        model: "qwen3.7-plus",
        displayName: "Qwen3.7 Plus",
        contextWindow: 1000000,
      },
      {
        model: "qwen3.6-plus",
        displayName: "Qwen3.6 Plus",
        contextWindow: 1000000,
      },
    ]),
    category: "cn_official",
    icon: "qwen",
    iconColor: "#6336E7",
  },
  {
    name: "QwenCloud For Coding",
    websiteUrl: "https://www.qwencloud.com",
    apiKeyUrl: "https://home.qwencloud.com/api-keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "qwencloud_coding",
      "https://coding-intl.dashscope.aliyuncs.com/v1",
      "qwen3.7-plus",
    ),
    endpointCandidates: ["https://coding-intl.dashscope.aliyuncs.com/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "qwen3.7-plus",
        displayName: "Qwen3.7 Plus",
        contextWindow: 1000000,
      },
      {
        model: "qwen3.6-plus",
        displayName: "Qwen3.6 Plus",
        contextWindow: 1000000,
      },
      {
        model: "qwen3-coder-plus",
        displayName: "Qwen3 Coder Plus",
        contextWindow: 131072,
        inputModalities: ["text"],
      },
    ]),
    category: "cn_official",
    icon: "qwen",
    iconColor: "#6336E7",
  },
  {
    name: "QwenCloud Token Plan",
    websiteUrl: "https://www.qwencloud.com",
    apiKeyUrl: "https://home.qwencloud.com/api-keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "qwencloud_token_plan",
      "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
      "qwen3.8-max",
    ),
    endpointCandidates: [
      "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
    ],
    apiFormat: "openai_responses",
    // 档位与窗口照抄官方 Codex model-catalogs.json（qwen3.8 系只收
    // low/medium/xhigh，默认 xhigh；无 high 档，勿按常规四档补齐）
    modelCatalog: modelCatalog([
      {
        model: "qwen3.8-max",
        displayName: "Qwen3.8 Max",
        contextWindow: 983616,
        supportsParallelToolCalls: false,
        reasoningLevels: ["low", "medium", "xhigh"],
        defaultReasoningLevel: "xhigh",
      },
      {
        model: "qwen3.8-flash",
        displayName: "Qwen3.8 Flash",
        contextWindow: 983616,
        supportsParallelToolCalls: false,
        reasoningLevels: ["low", "medium", "xhigh"],
        defaultReasoningLevel: "xhigh",
      },
      {
        model: "qwen3.7-max",
        displayName: "Qwen3.7 Max",
        contextWindow: 1000000,
        inputModalities: ["text"],
      },
    ]),
    category: "cn_official",
    icon: "qwen",
    iconColor: "#6336E7",
  },
  {
    name: "Tencent Hunyuan",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/apikey",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "hy3_tokenhub",
      "https://tokenhub.tencentmaas.com/v1",
      "hy3",
    ),
    // 官方备用域名 tencentmaas.cn（文档 1823/130078）；国际站 tokenhub-intl
    // 属不同地域，API Key 不跨站通用，不作候选
    endpointCandidates: [
      "https://tokenhub.tencentmaas.com/v1",
      "https://tokenhub.tencentmaas.cn/v1",
    ],
    // 腾讯 TokenHub 官方 Codex 文档（cloud.tencent.com/document/product/1823/133532）：
    // hy3 原生 Responses（wire_api=responses；官方硬性要求的
    // disable_response_storage=true 已由 generateThirdPartyConfig 输出）。
    // ⚠️ 须用 TokenHub API Key（创建时范围需勾选 Hy3）；Coding Plan / Token Plan
    // 订阅 Key 只能走各自 chat 端点，对本预设的 /v1 不通。
    // hy3 在带 tools 的请求里会把 reasoning_effort=low 服务端自动升为 high
    // （Codex 恒带 tools），默认 high 即真实行为。
    apiFormat: "openai_responses",
    // 无官方 catalog：合成 MiMo 式（shell_command 编辑、不发 freeform apply_patch）
    modelCatalog: modelCatalog([
      {
        model: "hy3",
        displayName: "Hy3",
        contextWindow: 256000,
        // hy3 不在官方图片理解模型名单（1823/136956），纯文本
        inputModalities: ["text"],
        // 官方档位枚举只有 low/high（1823/131208 + 开源权重 chat template
        // 对其他 effort 值直接 raise）；带 tools 时 low 被服务端升为 high
        reasoningLevels: ["low", "high"],
      },
      {
        model: "hy3-preview",
        displayName: "Hy3 Preview",
        contextWindow: 256000,
        inputModalities: ["text"],
        // 同 hy3：官方枚举 low/high（1823/130930 交错式思考模式文档）
        reasoningLevels: ["low", "high"],
      },
    ]),
    category: "cn_official",
    icon: "hunyuan",
    iconColor: "#0055E9",
  },
  {
    // 腾讯云 Token Plan 个人版（1823/130060，2026-08-21 版）：通用 + Hy 两
    // 系列共用同一端点与 API Key，catalog 合并两系列；Auto 智能路由的调用
    // ID 是 tc-code-latest。kimi-k2.5 官方标注 2026-08-31 下线不收（真 Key
    // 实测仍通，过期即弃）。minimax-m2.5 不在套餐文档表内、但 /plan/v3/models
    // 收录且真 Key 实测可用，照实收录。
    // 注意与 TokenHub 按量 API 市场（1823 线，Hunyuan 预设的 /v1 端点）是
    // 两条产品线：订阅 Key 只能走 /plan 端点，TokenHub Key 对 /plan 不通
    name: "Tencent Token Plan",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "tencent_token_plan",
      "https://api.lkeap.cloud.tencent.com/plan/v3",
      "tc-code-latest",
    ),
    endpointCandidates: ["https://api.lkeap.cloud.tencent.com/plan/v3"],
    // Token Plan 仅提供 Chat Completions；Codex 需要本地路由转换
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      // 阵容=个人版文档（2026-08-21 版）+ /models 实测；窗口=平台型号列表页
      // （intl 1300/78934，2026-08-28 版）口径。思考档位全部真 Key 实测
      // （2026-08-31）：thinking 开关对 tc-code-latest/deepseek/GLM/hy3
      // 真实生效；minimax-m2.5/m2.7 关不掉（参数被静默忽略，非报错），
      // 只列 high 防"选 none 却照样思考"的假档
      {
        model: "tc-code-latest",
        displayName: "Auto",
        // Auto 的窗口只有官方 OpenClaw 接入页给出（1823/130062、
        // 1300/81503）：196608；不写则后端回落 128K 默认值
        contextWindow: 196608,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-flash-202605",
        displayName: "DeepSeek V4 Flash",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro-202606",
        displayName: "DeepSeek V4 Pro",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "minimax-m2.7",
        displayName: "MiniMax M2.7",
        contextWindow: 200000,
        reasoningLevels: ["high"],
      },
      {
        // 不在套餐文档表，/models 收录 + 真 Key 实测可用（2026-08-31）
        model: "minimax-m2.5",
        displayName: "MiniMax M2.5",
        contextWindow: 200000,
        reasoningLevels: ["high"],
      },
      {
        model: "glm-5",
        displayName: "GLM-5",
        contextWindow: 200000,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "glm-5.1",
        displayName: "GLM-5.1",
        contextWindow: 200000,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "glm-5.2",
        displayName: "GLM-5.2",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      // Hy Token Plan 系列（与通用系列共用端点与 Key）；hy3-preview
      // 调用自动路由至 hy3（官方公告）；纯文本（1823/136956 名单无 hy3）
      {
        model: "hy3",
        displayName: "Hy3",
        contextWindow: 256000,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "hy3-preview",
        displayName: "Hy3 Preview",
        contextWindow: 256000,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
    ]),
    // 真 Key 实测（2026-08-31）：thinking 参数在 /plan 端点真实生效
    // （开/关均验证，thinking 文档 1300/80637 覆盖 /plan）；reasoning_effort
    // 全模型容忍不报错（含默认 high）。effortValueMode 不声明=passthrough，
    // 档位值域已由各模型 reasoningLevels 限定为实测安全集
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // 国际站（新加坡地域）个人版（intl 1300/81315，2026-08-20 版）：
    // Auto 调用 ID 是 auto（≠国内个人版 tc-code-latest），阵容与国内不同
    // （无 GLM-5/5.1/Hy3，多 GLM-5.2/MiniMax-M3）。端点用国际站文档钦定的
    // tencentcloudmaas.com 域（DNS 实测解析新加坡节点）；国内站文档对新加坡
    // 地域给的是 tokenhub-intl.tencentmaas.com，Key 按站独立不跨站通用，
    // 故互不作候选
    name: "Tencent Token Plan (Intl)",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "tencent_token_plan_intl",
      "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      "auto",
    ),
    endpointCandidates: ["https://tokenhub-intl.tencentcloudmaas.com/plan/v3"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      // 思考档位真 Key 实测（2026-08-31）：INTL auto 的 thinking 开关真实
      // 生效（与国内 auto 忽略关思考不同）；minimax-m3 关思考生效
      {
        model: "auto",
        displayName: "Auto",
        // Auto 的窗口只有官方 OpenClaw 接入页给出（1823/130062、
        // 1300/81503）：196608；不写则后端回落 128K 默认值
        contextWindow: 196608,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "glm-5.2",
        displayName: "GLM-5.2",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "kimi-k2.6",
        displayName: "Kimi K2.6",
        contextWindow: 262144,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro-202606",
        displayName: "DeepSeek V4 Pro",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-flash-202605",
        displayName: "DeepSeek V4 Flash",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "minimax-m3",
        displayName: "MiniMax M3",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
    ]),
    // thinking/reasoning_effort 实测同国内个人版（全模型容忍、开关生效）
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // Token Plan 企业版专业套餐（1823/130659，2026-08-25 版，广州地域）：
    // kimi-k2.5 官方标注 2026-08-31 下线不收；minimax-m2.5 型号列表已除名
    // 但真 Key 实测仍可用（2026-08-31），照实收录。新加坡地域阵容不同且
    // Key 不跨站，见 (Intl) 预设
    name: "Tencent Token Plan Enterprise Pro",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan-e",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "tencent_token_plan_enterprise_pro",
      "https://tokenhub.tencentmaas.com/plan/v3",
      "auto",
    ),
    // 广州地域为默认端点；国内站企业套餐另可选新加坡地域（1823/130659、
    // 131173 双地域表：tokenhub-intl.tencentmaas.com，需开通新加坡地域，
    // 不支持跨地域调用，故仅作候选端点）
    endpointCandidates: [
      "https://tokenhub.tencentmaas.com/plan/v3",
      "https://tokenhub-intl.tencentmaas.com/plan/v3",
    ],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      // 阵容与排序=企业专业版文档广州标签页（2026-08-25 版）。思考档位
      // 全部真 Key 实测（2026-08-31）：glm-5.3 始终思考且档位严格枚举
      // low/high/max（medium/xhigh 直接 400，错误信息即枚举来源）；
      // kimi-k2.7-code(-highspeed) 仅接受 thinking:enabled；
      // minimax-m2.5/m2.7 关思考被静默忽略；国内 auto 同样忽略关思考；
      // 其余模型 thinking 开关真实生效
      {
        model: "auto",
        displayName: "Auto",
        // Auto 的窗口只有官方 OpenClaw 接入页给出（1823/130062、
        // 1300/81503）：196608；不写则后端回落 128K 默认值
        contextWindow: 196608,
        reasoningLevels: ["high"],
      },
      {
        model: "glm-5.3",
        displayName: "GLM-5.3",
        contextWindow: 1048576,
        reasoningLevels: ["low", "high", "max"],
        // 显式默认 high：模板默认 medium 不在严格枚举内会被丢弃，回落
        // canonical.last()=max——最慢最耗额度，非厂商钦定默认（真 Key 实测
        // 未发现无参默认值证据），按预算套餐取向选 high
        defaultReasoningLevel: "high",
      },
      {
        model: "glm-5.2",
        displayName: "GLM-5.2",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "glm-5",
        displayName: "GLM-5",
        contextWindow: 200000,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "glm-5.1",
        displayName: "GLM-5.1",
        contextWindow: 200000,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "glm-5-turbo",
        displayName: "GLM-5 Turbo",
        contextWindow: 200000,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "kimi-k2.7-code",
        displayName: "Kimi K2.7 Code",
        contextWindow: 262144,
        reasoningLevels: ["high"],
      },
      {
        model: "kimi-k2.7-code-highspeed",
        displayName: "Kimi K2.7 Code HighSpeed",
        contextWindow: 262144,
        reasoningLevels: ["high"],
      },
      {
        model: "kimi-k2.6",
        displayName: "Kimi K2.6",
        contextWindow: 262144,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "minimax-m2.7",
        displayName: "MiniMax M2.7",
        contextWindow: 200000,
        reasoningLevels: ["high"],
      },
      {
        model: "minimax-m3",
        displayName: "MiniMax M3",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        // 型号列表已除名，真 Key 实测仍可用（2026-08-31）
        model: "minimax-m2.5",
        displayName: "MiniMax M2.5",
        contextWindow: 200000,
        reasoningLevels: ["high"],
      },
      {
        model: "deepseek-v4-flash",
        displayName: "DeepSeek V4 Flash",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro",
        displayName: "DeepSeek V4 Pro",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-flash-0731",
        displayName: "DeepSeek V4 Flash 0731 GA",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro-0813",
        displayName: "DeepSeek V4 Pro 0813 GA",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-flash-202605",
        displayName: "DeepSeek V4 Flash Official",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro-202606",
        displayName: "DeepSeek V4 Pro Official",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
    ]),
    // reasoning_effort 默认 high 全模型实测容忍；glm-5.3 的 medium/xhigh
    // 会 400，档位值域已由各模型 reasoningLevels 限定为实测安全集
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // 国际站企业版专业套餐（intl 1300/81489，2026-08-26 版，新加坡地域）：
    // 阵容为广州地域子集（无 GLM-5/5.1/5-Turbo、Kimi-K2.6、MiniMax-M2.7）
    name: "Tencent Token Plan Enterprise Pro (Intl)",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan-e",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "tencent_token_plan_enterprise_pro_intl",
      "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      "auto",
    ),
    // 新加坡地域为默认端点；国际站企业套餐另可选广州地域（1300/81489、
    // 81490 双地域表：tokenhub.tencentcloudmaas.com，需开通广州地域，
    // 不支持跨地域调用，故仅作候选端点）
    endpointCandidates: [
      "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      "https://tokenhub.tencentcloudmaas.com/plan/v3",
    ],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      // 阵容与排序=国际站企业专业版文档（新加坡地域）。思考档位真 Key
      // 实测（2026-08-31）：INTL auto 关思考真实生效（≠国内 auto 忽略）；
      // glm-5.3 严格枚举 low/high/max；kimi-k2.7-code(-highspeed) 仅
      // thinking:enabled；其余开关生效
      {
        model: "auto",
        displayName: "Auto",
        // Auto 的窗口只有官方 OpenClaw 接入页给出（1823/130062、
        // 1300/81503）：196608；不写则后端回落 128K 默认值
        contextWindow: 196608,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "glm-5.3",
        displayName: "GLM-5.3",
        contextWindow: 1048576,
        reasoningLevels: ["low", "high", "max"],
        // 显式默认 high：模板默认 medium 不在严格枚举内会被丢弃，回落
        // canonical.last()=max——最慢最耗额度，非厂商钦定默认（真 Key 实测
        // 未发现无参默认值证据），按预算套餐取向选 high
        defaultReasoningLevel: "high",
      },
      {
        model: "glm-5.2",
        displayName: "GLM-5.2",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "minimax-m3",
        displayName: "MiniMax M3",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "kimi-k2.7-code",
        displayName: "Kimi K2.7 Code",
        contextWindow: 262144,
        reasoningLevels: ["high"],
      },
      {
        model: "kimi-k2.7-code-highspeed",
        displayName: "Kimi K2.7 Code HighSpeed",
        contextWindow: 262144,
        reasoningLevels: ["high"],
      },
      {
        model: "deepseek-v4-flash",
        displayName: "DeepSeek V4 Flash",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro",
        displayName: "DeepSeek V4 Pro",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-flash-0731",
        displayName: "DeepSeek V4 Flash 0731 GA",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro-0813",
        displayName: "DeepSeek V4 Pro 0813 GA",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-flash-202605",
        displayName: "DeepSeek V4 Flash Official",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro-202606",
        displayName: "DeepSeek V4 Pro Official",
        contextWindow: 1048576,
        reasoningLevels: ["none", "high"],
      },
    ]),
    // reasoning_effort 默认 high 全模型实测容忍；同国内企业专业版
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // Token Plan 企业版轻享套餐（1823/131173，2026-08-28 版）：仅 Auto 模型。
    // 国内 auto 关思考被静默忽略（真 Key 实测 2026-08-31），只列 high
    name: "Tencent Token Plan Enterprise Lite",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan-e",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "tencent_token_plan_enterprise_lite",
      "https://tokenhub.tencentmaas.com/plan/v3",
      "auto",
    ),
    // 广州地域为默认端点；国内站企业套餐另可选新加坡地域（1823/130659、
    // 131173 双地域表：tokenhub-intl.tencentmaas.com，需开通新加坡地域，
    // 不支持跨地域调用，故仅作候选端点）
    endpointCandidates: [
      "https://tokenhub.tencentmaas.com/plan/v3",
      "https://tokenhub-intl.tencentmaas.com/plan/v3",
    ],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "auto",
        displayName: "Auto",
        // Auto 的窗口只有官方 OpenClaw 接入页给出（1823/130062、
        // 1300/81503）：196608；不写则后端回落 128K 默认值
        contextWindow: 196608,
        reasoningLevels: ["high"],
      },
    ]),
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // 国际站企业版轻享套餐（intl 1300/81490）：新加坡地域（资源调度范围
    // Global），仅 Auto 模型。INTL auto 关思考真实生效（真 Key 实测）
    name: "Tencent Token Plan Enterprise Lite (Intl)",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan-e",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "tencent_token_plan_enterprise_lite_intl",
      "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      "auto",
    ),
    // 新加坡地域为默认端点；国际站企业套餐另可选广州地域（1300/81489、
    // 81490 双地域表：tokenhub.tencentcloudmaas.com，需开通广州地域，
    // 不支持跨地域调用，故仅作候选端点）
    endpointCandidates: [
      "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      "https://tokenhub.tencentcloudmaas.com/plan/v3",
    ],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "auto",
        displayName: "Auto",
        // Auto 的窗口只有官方 OpenClaw 接入页给出（1823/130062、
        // 1300/81503）：196608；不写则后端回落 128K 默认值
        contextWindow: 196608,
        reasoningLevels: ["none", "high"],
      },
    ]),
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    name: "StepFun",
    websiteUrl: "https://platform.stepfun.com/step-plan",
    apiKeyUrl: "https://platform.stepfun.com/interface-key",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "stepfun",
      "https://api.stepfun.com/step_plan/v1",
      "step-3.7-flash",
    ),
    endpointCandidates: ["https://api.stepfun.com/step_plan/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      // 档位照抄官方两站模型页与 reasoning 指南（2026-08-15 盘点）：
      // 3.7-flash 三档默认 medium、2603 两档；无后缀 3.5-flash 官方未暴露
      // effort，不填。全系无关思考形态，none 一律不列。effort 下发由后端
      // 按模型推断（2603=low_high 收敛、3.7=passthrough），预设不加
      // codexChatReasoning——显式声明是 provider 级会丢 per-model 门控
      {
        model: "step-3.7-flash",
        displayName: "Step 3.7 Flash",
        contextWindow: 262144,
        reasoningLevels: ["low", "medium", "high"],
      },
      {
        model: "step-3.5-flash-2603",
        displayName: "Step 3.5 Flash 2603",
        contextWindow: 262144,
        reasoningLevels: ["low", "high"],
      },
      {
        model: "step-3.5-flash",
        displayName: "Step 3.5 Flash",
        contextWindow: 262144,
      },
    ]),
    category: "cn_official",
    icon: "stepfun",
    iconColor: "#16D6D2",
  },
  {
    name: "StepFun en",
    websiteUrl: "https://platform.stepfun.ai/step-plan",
    apiKeyUrl: "https://platform.stepfun.ai/interface-key",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "stepfun_en",
      "https://api.stepfun.ai/step_plan/v1",
      "step-3.7-flash",
    ),
    endpointCandidates: ["https://api.stepfun.ai/step_plan/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      // 档位照抄官方两站模型页与 reasoning 指南（2026-08-15 盘点）：
      // 3.7-flash 三档默认 medium、2603 两档；无后缀 3.5-flash 官方未暴露
      // effort，不填。全系无关思考形态，none 一律不列。effort 下发由后端
      // 按模型推断（2603=low_high 收敛、3.7=passthrough），预设不加
      // codexChatReasoning——显式声明是 provider 级会丢 per-model 门控
      {
        model: "step-3.7-flash",
        displayName: "Step 3.7 Flash",
        contextWindow: 262144,
        reasoningLevels: ["low", "medium", "high"],
      },
      {
        model: "step-3.5-flash-2603",
        displayName: "Step 3.5 Flash 2603",
        contextWindow: 262144,
        reasoningLevels: ["low", "high"],
      },
      {
        model: "step-3.5-flash",
        displayName: "Step 3.5 Flash",
        contextWindow: 262144,
      },
    ]),
    category: "cn_official",
    icon: "stepfun",
    iconColor: "#16D6D2",
  },
  {
    name: "ModelScope",
    websiteUrl: "https://modelscope.cn",
    apiKeyUrl: "https://modelscope.cn/my/myaccesstoken",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "modelscope",
      "https://api-inference.modelscope.cn/v1",
      "ZhipuAI/GLM-5.2",
    ),
    endpointCandidates: ["https://api-inference.modelscope.cn/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        // 2026-08-15 盘点：GLM-5.1 裸 id 不在 ModelScope 免费 API-Inference
        // 43 条服务清单（只有需自托管阿里云密钥的 :DashScope 变体），换 GLM-5.2
        //（在清单内、免费"魔搭社区"路由）。档位不填：ModelScope 是否把思考
        // 字段透传给上游未证实
        model: "ZhipuAI/GLM-5.2",
        displayName: "ZhipuAI / GLM-5.2",
        contextWindow: 200000,
      },
    ]),
    // 平台方言修正（2026-08-15 盘点）：thinking:{type} 是智谱自家端点形态，
    // ModelScope 平台文档零出现；平台真实开关=顶层 enable_thinking 布尔
    //（官方模型页 extra_body 范例+百炼 GLM 一手文档双证）。整块必须保留——
    // 删掉会落到后端 glm 模型名推断、错误方言原地复活
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: false,
      thinkingParam: "enable_thinking",
      effortParam: "none",
      outputFormat: "reasoning_content",
    },
    category: "aggregator",
    icon: "modelscope",
    iconColor: "#624AFF",
  },
  {
    name: "Longcat",
    websiteUrl: "https://longcat.chat/platform",
    apiKeyUrl: "https://longcat.chat/platform/api_keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "longcat",
      "https://api.longcat.chat/openai/v1",
      "LongCat-2.0",
    ),
    endpointCandidates: ["https://api.longcat.chat/openai/v1"],
    // 美团 LongCat 官方 Codex 文档用 wire_api=responses 对自家 base_url，原生 Responses，无需路由接管转换
    apiFormat: "openai_responses",
    // 无官方 catalog：合成 MiMo 式（shell_command 编辑、不发 freeform apply_patch）。
    // 注：LongCat 的 /responses 工具类型契约文档化程度最低，建议真机冒烟一次
    modelCatalog: modelCatalog([
      {
        model: "LongCat-2.0",
        displayName: "LongCat 2.0",
        contextWindow: 1048576,
        // LongCat 无档位可调：全站唯一 effort 证据=官方 Codex 示例的 high；
        // 关思考走另一字段 thinking:{type:disabled}（effort 拼写无文档），
        // models API 的 supported_parameters 也不含 reasoning，故不提供 none 假开关
        reasoningLevels: ["high"],
      },
    ]),
    category: "cn_official",
    icon: "longcat",
    iconColor: "#29E154",
  },
  {
    name: "MiniMax",
    websiteUrl: "https://platform.minimaxi.com",
    apiKeyUrl: "https://platform.minimaxi.com/subscribe/coding-plan",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "minimax",
      "https://api.minimaxi.com/v1",
      "MiniMax-M3",
    ),
    endpointCandidates: ["https://api.minimaxi.com/v1"],
    // MiniMax 官方 API 参考已列 /v1/responses 为正式端点（CN/intl 双区，POST /v1/responses），原生 Responses，无需路由接管转换
    apiFormat: "openai_responses",
    // 官方 Codex catalog（platform.minimaxi.com/docs/token-plan/codex-cli）：
    // shell_command 编辑、并行工具、文本+图像，不声明 freeform apply_patch。
    // 档位照抄官方 catalog：none/high（M3 的 effort 是思考开关，minimal/low/medium
    // 端点接受但与 high 行为完全等价，不给假差异档）。与模板默认一致故 Codex 侧
    // 零行为变化，显式声明只为表单可见（"未设置"误导性更大，Jason 2026-08-15 拍板）
    modelCatalog: modelCatalog([
      {
        model: "MiniMax-M3",
        displayName: "MiniMax-M3",
        contextWindow: 1000000,
        reasoningLevels: ["none", "high"],
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        baseInstructions:
          "You are Codex, a coding agent based on MiniMax-M3. You and the user share the same workspace and collaborate to achieve the user's goals.",
      },
    ]),
    category: "cn_official",
    partnerPromotionKey: "minimax_cn",
    theme: {
      backgroundColor: "#f64551",
      textColor: "#FFFFFF",
    },
    icon: "minimax",
    iconColor: "#FF6B6B",
  },
  {
    name: "MiniMax en",
    websiteUrl: "https://platform.minimax.io",
    apiKeyUrl: "https://platform.minimax.io/subscribe/coding-plan",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "minimax_en",
      "https://api.minimax.io/v1",
      "MiniMax-M3",
    ),
    endpointCandidates: ["https://api.minimax.io/v1"],
    // MiniMax 官方 API 参考已列 /v1/responses 为正式端点（CN/intl 双区，POST /v1/responses），原生 Responses，无需路由接管转换
    apiFormat: "openai_responses",
    // 官方 Codex catalog（platform.minimax.io/docs/token-plan/codex）：
    // shell_command 编辑、并行工具、文本+图像，不声明 freeform apply_patch。
    // 档位照抄官方 catalog：none/high（M3 的 effort 是思考开关，minimal/low/medium
    // 端点接受但与 high 行为完全等价，不给假差异档）。与模板默认一致故 Codex 侧
    // 零行为变化，显式声明只为表单可见（"未设置"误导性更大，Jason 2026-08-15 拍板）
    modelCatalog: modelCatalog([
      {
        model: "MiniMax-M3",
        displayName: "MiniMax-M3",
        contextWindow: 1000000,
        reasoningLevels: ["none", "high"],
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        baseInstructions:
          "You are Codex, a coding agent based on MiniMax-M3. You and the user share the same workspace and collaborate to achieve the user's goals.",
      },
    ]),
    category: "cn_official",
    partnerPromotionKey: "minimax_en",
    theme: {
      backgroundColor: "#f64551",
      textColor: "#FFFFFF",
    },
    icon: "minimax",
    iconColor: "#FF6B6B",
  },
  {
    name: "BaiLing",
    websiteUrl: "https://alipaytbox.yuque.com/sxs0ba/ling/get_started",
    apiKeyUrl: "https://ling.tbox.cn/open",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "bailing",
      "https://api.tbox.cn/api/llm/v1",
      "Ling-2.6-1T",
    ),
    endpointCandidates: ["https://api.tbox.cn/api/llm/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "Ling-2.6-1T",
        displayName: "Ling-2.6-1T",
        contextWindow: 262144,
      },
    ]),
    category: "cn_official",
  },
  {
    name: "Xiaomi MiMo",
    websiteUrl: "https://platform.xiaomimimo.com",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/api-keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "xiaomi_mimo",
      "https://api.xiaomimimo.com/v1",
      "mimo-v2.5-pro",
    ),
    endpointCandidates: ["https://api.xiaomimimo.com/v1"],
    // 小米 MiMo 官方 Codex 文档已声明原生支持 Responses API（wire_api=responses 对自家 base_url），无需路由接管转换
    apiFormat: "openai_responses",
    // 官方 Codex catalog（mimo.mi.com/.../codex-configuration）：
    // shell_command 编辑、不声明 freeform apply_patch。
    // 档位照抄官方 catalog：none/high（端点另收 low/medium 但官方自述三档
    // "效果一致，暂不区分推理强度"，不给假差异档）。与模板默认一致故 Codex 侧
    // 零行为变化，显式声明只为表单可见（"未设置"误导性更大，Jason 2026-08-15 拍板）
    modelCatalog: modelCatalog([
      {
        model: "mimo-v2.5-pro",
        displayName: "MiMo V2.5 Pro",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
        baseInstructions:
          "You are MiMo, an AI assistant developed by Xiaomi. Today's date: {date} {week}. Your knowledge cutoff date is December 2024.",
      },
      {
        model: "mimo-v2.5",
        displayName: "MiMo V2.5",
        contextWindow: 1048576,
        inputModalities: ["text", "image"],
        reasoningLevels: ["none", "high"],
        baseInstructions:
          "You are MiMo, an AI assistant developed by Xiaomi. Today's date: {date} {week}. Your knowledge cutoff date is December 2024.",
      },
    ]),
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
  },
  {
    name: "Xiaomi MiMo Token Plan (China)",
    websiteUrl: "https://platform.xiaomimimo.com/#/token-plan",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/plan-manage",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "xiaomi_mimo_token_plan",
      "https://token-plan-cn.xiaomimimo.com/v1",
      "mimo-v2.5-pro",
    ),
    endpointCandidates: ["https://token-plan-cn.xiaomimimo.com/v1"],
    // 小米 MiMo 官方 Codex 文档已声明原生支持 Responses API（wire_api=responses 对自家 base_url），无需路由接管转换
    apiFormat: "openai_responses",
    // 官方 Codex catalog（mimo.mi.com/.../codex-configuration）：
    // shell_command 编辑、不声明 freeform apply_patch。
    // 档位照抄官方 catalog：none/high（端点另收 low/medium 但官方自述三档
    // "效果一致，暂不区分推理强度"，不给假差异档）。与模板默认一致故 Codex 侧
    // 零行为变化，显式声明只为表单可见（"未设置"误导性更大，Jason 2026-08-15 拍板）
    modelCatalog: modelCatalog([
      {
        model: "mimo-v2.5-pro",
        displayName: "MiMo V2.5 Pro",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
        baseInstructions:
          "You are MiMo, an AI assistant developed by Xiaomi. Today's date: {date} {week}. Your knowledge cutoff date is December 2024.",
      },
      {
        model: "mimo-v2.5",
        displayName: "MiMo V2.5",
        contextWindow: 1048576,
        inputModalities: ["text", "image"],
        reasoningLevels: ["none", "high"],
        baseInstructions:
          "You are MiMo, an AI assistant developed by Xiaomi. Today's date: {date} {week}. Your knowledge cutoff date is December 2024.",
      },
    ]),
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
  },
  {
    name: "Novita AI",
    websiteUrl: "https://novita.ai",
    apiKeyUrl: "https://novita.ai",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "novita",
      "https://api.novita.ai/openai/v1",
      "zai-org/glm-5.1",
    ),
    endpointCandidates: ["https://api.novita.ai/openai/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      // 平台真开关=顶层 enable_thinking 布尔（Novita API 参考+FAQ 双证）→
      // 两态。glm-5.1 响应该字段是"官方替代公告+Z.AI 模型卡"两跳推断，无逐字
      // 直证（2026-08-15 盘点，中置信；同平台 MiniMax-M1 是不可关思考的反例）
      {
        model: "zai-org/glm-5.1",
        displayName: "GLM-5.1",
        contextWindow: 202800,
        reasoningLevels: ["none", "high"],
      },
    ]),
    // 方言修正（2026-08-15 盘点）：thinking:{type} 是 Z.AI 自家端点形态，
    // Novita「Create chat completion」参数枚举与全站文档零出现
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: false,
      thinkingParam: "enable_thinking",
      effortParam: "none",
      outputFormat: "reasoning_content",
    },
    category: "aggregator",
    icon: "novita",
    iconColor: "#000000",
  },
  {
    name: "xAI (Grok)",
    websiteUrl: "https://x.ai/api",
    apiKeyUrl: "https://console.x.ai",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig("xai", "https://api.x.ai/v1", "grok-4.5"),
    endpointCandidates: ["https://api.x.ai/v1"],
    // xAI 官方以 /v1/responses 为一等端点（docs.x.ai api-reference）：Codex 硬依赖的
    // store:false / include=["reasoning.encrypted_content"] / reasoning effort 均支持，
    // 原生 Responses，无需路由接管转换
    apiFormat: "openai_responses",
    modelCatalog: modelCatalog([
      {
        model: "grok-4.5",
        displayName: "Grok 4.5",
        contextWindow: 500000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        // 实测（2026-08-30，native /v1/responses 逐档探测）：grok-4.5 接受
        // low/medium/high/xhigh，拒绝 max（HTTP 400 "Invalid reasoning
        // effort"）；"Reasoning cannot be disabled" 故无 none 档。Codex 不按
        // catalog clamp 越界档位（Desktop UI 选出的 max 会原样发出），此列表
        // 必须与上游实收集合一致，勿凭文档增删。
        // ⚠️ docs.x.ai/developers/grok-4-5 页面实际渲染的是 grok-4.6 内容勿引
        reasoningLevels: ["low", "medium", "high", "xhigh"],
      },
    ]),
    category: "third_party",
    icon: "xai",
    iconColor: "#000000",
  },
  {
    name: "xAI (Grok) OAuth",
    websiteUrl: "https://x.ai/grok",
    auth: generateThirdPartyAuth(""),
    // 托管 OAuth：真实 token 由本地代理按请求注入，CodexAdapter 硬定向
    // api.x.ai；这里的 base_url / 空 auth 只是配置快照，转发时不生效。
    // requires_openai_auth 必须是 false：keyless + true 会被后端安全闸
    // 拒绝切换（后端写入层对存量卡也会强制归一为 false）。
    config: generateThirdPartyConfig("xai", "https://api.x.ai/v1", "grok-4.5", {
      requiresOpenAiAuth: false,
    }),
    apiFormat: "openai_responses",
    providerType: "xai_oauth",
    requiresOAuth: true,
    modelCatalog: modelCatalog([
      {
        model: "grok-4.5",
        displayName: "Grok 4.5",
        contextWindow: 500000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        // 实测（2026-08-30，native /v1/responses 逐档探测）：grok-4.5 接受
        // low/medium/high/xhigh，拒绝 max（HTTP 400 "Invalid reasoning
        // effort"）；"Reasoning cannot be disabled" 故无 none 档。Codex 不按
        // catalog clamp 越界档位（Desktop UI 选出的 max 会原样发出），此列表
        // 必须与上游实收集合一致，勿凭文档增删。
        // ⚠️ docs.x.ai/developers/grok-4-5 页面实际渲染的是 grok-4.6 内容勿引
        reasoningLevels: ["low", "medium", "high", "xhigh"],
      },
    ]),
    category: "third_party",
    icon: "xai",
    iconColor: "#000000",
  },
  {
    name: "Nvidia",
    websiteUrl: "https://build.nvidia.com",
    apiKeyUrl: "https://build.nvidia.com/settings/api-keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "nvidia",
      "https://integrate.api.nvidia.com/v1",
      "moonshotai/kimi-k2.5",
    ),
    endpointCandidates: ["https://integrate.api.nvidia.com/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "moonshotai/kimi-k2.5",
        displayName: "Kimi K2.5",
        contextWindow: 262144,
      },
    ]),
    // 假开关撤销（2026-08-15 盘点）：NIM 官方 OpenAPI（moonshotai-kimi-k2-5-infer）
    // 请求体 additionalProperties:false 且合法字段表无顶层 thinking——原
    // thinking:{type} 注入要么被吞要么直接被拒；真参数 chat_template_kwargs:
    // {thinking:bool} 不在 thinkingParam 值域内。⚠️整块保留、thinkingParam
    // 显式置 none：删块会让后端推断按模型名命中 kimi 分支、假开关原地复活
    codexChatReasoning: {
      supportsThinking: false,
      supportsEffort: false,
      thinkingParam: "none",
      effortParam: "none",
      outputFormat: "reasoning_content",
    },
    category: "aggregator",
    icon: "nvidia",
    iconColor: "#000000",
  },
  {
    name: "OpenCode Go",
    websiteUrl: "https://opencode.ai/go",
    apiKeyUrl: "https://opencode.ai/go?ref=2YTRG2NGTX",
    partnerPromotionKey: "opencode_go",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "opencode_go",
      "https://opencode.ai/zen/go/v1",
      "glm-5.2",
    ),
    endpointCandidates: ["https://opencode.ai/zen/go/v1"],
    apiFormat: "openai_chat",
    // OpenCode Zen 网关：统一接受顶层 reasoning_effort（其自家客户端同款参数），
    // 但合法档位逐模型（见各条目 reasoningLevels，镜像 models.dev；opencode
    // 客户端同样严格按模型声明发值）——代理转换层按表钳制，未声明 effort 的
    // 模型（toggle 型如 glm-5.1）不发该字段。不发厂商原生 thinking 字段。
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "none",
      effortParam: "reasoning_effort",
      effortValueMode: "zen",
      outputFormat: "reasoning_content",
    },
    modelCatalog: modelCatalog([
      {
        model: "glm-5.2",
        displayName: "GLM 5.2",
        contextWindow: 204800,
        reasoningLevels: ["high", "max"],
      },
      { model: "glm-5.1", displayName: "GLM 5.1", contextWindow: 204800 },
      {
        model: "kimi-k2.7-code",
        displayName: "Kimi K2.7 Code",
        contextWindow: 262144,
      },
      {
        model: "deepseek-v4-pro",
        displayName: "DeepSeek V4 Pro",
        contextWindow: 1048576,
        reasoningLevels: ["high", "max"],
      },
      {
        model: "deepseek-v4-flash",
        displayName: "DeepSeek V4 Flash",
        contextWindow: 1048576,
        reasoningLevels: ["low", "high", "max"],
      },
      {
        model: "mimo-v2.5-pro",
        displayName: "MiMo V2.5 Pro",
        contextWindow: 1048576,
      },
    ]),
    category: "third_party",
    icon: "opencode",
    iconColor: "#211E1E",
  },
  {
    name: "AiHubMix",
    websiteUrl: "https://aihubmix.com",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "aihubmix",
      "https://aihubmix.com/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://aihubmix.com/v1",
      "https://api.aihubmix.com/v1",
    ],
    icon: "aihubmix",
    iconColor: "#006FFB",
  },
  {
    name: "CherryIN",
    websiteUrl: "https://open.cherryin.ai",
    apiKeyUrl: "https://open.cherryin.ai/console/token",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "cherryin",
      "https://open.cherryin.net/v1",
      "openai/gpt-5.6-sol",
    ),
    endpointCandidates: ["https://open.cherryin.net/v1"],
    category: "aggregator",
    icon: "cherryin",
  },
  {
    name: "RelaxyCode",
    websiteUrl: "https://www.relaxycode.com",
    apiKeyUrl: "https://www.relaxycode.com/register",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "relaxycode",
      "https://www.relaxycode.com/v1",
      "gpt-5.6-sol",
    ),
    icon: "relaxcode",
  },
  {
    name: "E-FlowCode",
    websiteUrl: "https://e-flowcode.cc",
    apiKeyUrl: "https://e-flowcode.cc",
    auth: {
      OPENAI_API_KEY: "",
    },
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
model_reasoning_effort = "high"
disable_response_storage = true
personality = "pragmatic"

[model_providers.custom]
name = "E-FlowCode"
base_url = "https://e-flowcode.cc/v1"
wire_api = "responses"
requires_openai_auth = true
model_context_window = 1000000
model_auto_compact_token_limit = 9000000`,
    category: "third_party",
    endpointCandidates: ["https://e-flowcode.cc/v1"],
    icon: "eflowcode",
    iconColor: "#000000",
  },
  {
    name: "PIPELLM",
    websiteUrl: "https://code.pipellm.ai",
    apiKeyUrl: "https://code.pipellm.ai/login?ref=uvw650za",
    auth: {
      OPENAI_API_KEY: "",
    },
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
model_reasoning_effort = "medium"
disable_response_storage = true

[model_providers.custom]
name = "PIPELLM"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://cc-api.pipellm.ai/v1"`,
    category: "aggregator",
    endpointCandidates: ["https://cc-api.pipellm.ai/v1"],
    icon: "pipellm",
  },
  {
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "openrouter",
      "https://openrouter.ai/api/v1",
      "gpt-5.6-sol",
    ),
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6566F1",
  },
  {
    name: "TheRouter",
    websiteUrl: "https://therouter.ai",
    apiKeyUrl: "https://dashboard.therouter.ai",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "therouter",
      "https://api.therouter.ai/v1",
      "openai/gpt-5.3-codex",
    ),
    endpointCandidates: ["https://api.therouter.ai/v1"],
    category: "aggregator",
  },
  {
    name: "JieKou AI",
    websiteUrl: "https://jiekou.ai/#model-library",
    apiKeyUrl: "https://jiekou.ai/settings/key-management",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "jiekou",
      "https://api.jiekou.ai/openai/v1",
      "claude-fable-5",
    ),
    endpointCandidates: ["https://api.jiekou.ai/openai/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "claude-fable-5",
        displayName: "Claude Fable 5",
        contextWindow: 1000000,
        inputModalities: ["text", "image"],
      },
    ]),
    category: "aggregator",
    icon: "jiekou",
    iconColor: "#000000",
  },
  {
    name: "AICodeWith",
    websiteUrl: "https://aicodewith.ai",
    apiKeyUrl: "https://aicodewith.ai/login?tab=register",
    auth: generateThirdPartyAuth(""),
    // 官方 Codex 专用端点，不能用通用 /v1（协议不同）
    config: generateThirdPartyConfig(
      "aicodewith",
      "https://api.aicodewith.ai/chatgpt/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.aicodewith.ai/chatgpt/v1"],
    category: "aggregator",
    icon: "aicodewith",
    iconColor: "#3A3B40",
  },
];
