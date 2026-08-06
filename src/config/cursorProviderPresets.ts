/**
 * Cursor IDE provider presets configuration
 *
 * Cursor 供应商预设独立维护（不依赖 codexProviderPresets），
 * 与 opencode/claude 等应用的预设文件保持同样的独立编写风格。
 */
import type { ProviderCategory, CodexCatalogModel } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

export interface CursorProviderPreset {
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: CursorProviderSettingsConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  templateValues?: Record<string, TemplateValueConfig>;
  theme?: PresetTheme;
  icon?: string;
  iconColor?: string;
  /** Optional model catalog for model name mapping */
  modelCatalog?: CodexCatalogModel[];
}

export interface CursorProviderSettingsConfig {
  baseUrl?: string;
  apiKey?: string;
  model?: string;
  [key: string]: unknown;
}

/**
 * Cursor Auto 模式使用的已知模型列表。
 * 新建非 official 供应商时预填充到模型映射表，用户只需填写右侧的"上游模型名"。
 * `model` = Cursor 侧的模型名，`displayName` 留空让用户填上游实际模型。
 */
export const CURSOR_AUTO_MODEL_IDS = [
  "auto",
  "Cursor Grok 4.5",
  "Composer 2.5",
  "Opus 5",
  "GPT-5.6 Sol",
  "Fable 5",
  "Sonnet 5",
  "GPT-5.6 Terra",
];

export function cursorDefaultCatalogModels(): CodexCatalogModel[] {
  return CURSOR_AUTO_MODEL_IDS.map((id) => ({
    model: id,
    displayName: "",
    contextWindow: "",
  }));
}

export const cursorProviderPresets: CursorProviderPreset[] = [
  {
    name: "Cursor Official",
    websiteUrl: "https://cursor.com",
    apiKeyUrl: "https://cursor.com/settings/api-keys",
    settingsConfig: {
      baseUrl: "",
      apiKey: "",
      model: "",
    },
    isOfficial: true,
    category: "official",
    icon: "cursor",
  },
  {
    name: "Anthropic",
    websiteUrl: "https://console.anthropic.com",
    apiKeyUrl: "https://console.anthropic.com/settings/keys",
    settingsConfig: {
      baseUrl: "https://api.anthropic.com",
      apiKey: "",
      model: "claude-sonnet-4-20250514",
    },
    category: "official",
    icon: "anthropic",
    iconColor: "#D97757",
    modelCatalog: cursorDefaultCatalogModels(),
  },
  {
    name: "OpenAI",
    websiteUrl: "https://platform.openai.com",
    apiKeyUrl: "https://platform.openai.com/api-keys",
    settingsConfig: {
      baseUrl: "https://api.openai.com/v1",
      apiKey: "",
      model: "gpt-4o",
    },
    category: "official",
    icon: "openai",
    iconColor: "#00A67E",
    modelCatalog: cursorDefaultCatalogModels(),
  },
  {
    name: "OpenAI Compatible",
    nameKey: "providerForm.presets.openaiCompatible",
    websiteUrl: "",
    settingsConfig: {
      baseUrl: "",
      apiKey: "",
      model: "",
    },
    category: "custom",
    icon: "openai",
    iconColor: "#00A67E",
    modelCatalog: cursorDefaultCatalogModels(),
  },
  // ===== 第三方 OpenAI 兼容供应商（独立维护） =====
  {
    name: "Kimi",
    websiteUrl: "https://platform.kimi.com?aff=cc-switch",
    apiKeyUrl: "https://platform.kimi.com/console/api-keys?aff=cc-switch",
    settingsConfig: {
      baseUrl: "https://api.moonshot.cn/v1",
      apiKey: "",
      model: "kimi-k2.7-code",
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
    modelCatalog: [
      {
        model: "kimi-k2.7-code",
        displayName: "Kimi K2.7 Code",
        contextWindow: 262144,
      },
      { model: "kimi-k3", displayName: "Kimi K3", contextWindow: 1048576 },
    ],
  },
  {
    name: "Kimi For Coding",
    websiteUrl: "https://www.kimi.com/code/?aff=cc-switch",
    apiKeyUrl: "https://www.kimi.com/code/?aff=cc-switch",
    settingsConfig: {
      baseUrl: "https://api.kimi.com/coding/v1",
      apiKey: "",
      model: "kimi-for-coding",
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
    modelCatalog: [
      {
        model: "kimi-for-coding",
        displayName: "Kimi For Coding",
        contextWindow: 262144,
      },
    ],
  },
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.com",
    apiKeyUrl: "https://www.packyapi.com/register?aff=cc-switch",
    settingsConfig: {
      baseUrl: "https://www.packyapi.com/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "third_party",
    icon: "packycode",
  },
  {
    name: "ZetaAPI",
    websiteUrl: "https://zetaapi.ai",
    apiKeyUrl: "https://zetaapi.ai/go/u117",
    settingsConfig: {
      baseUrl: "https://api.zetaapi.ai/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "zetaapi",
  },
  {
    name: "APINebula",
    websiteUrl: "https://apinebula.com",
    apiKeyUrl: "https://apinebula.com/VjM74M",
    settingsConfig: {
      baseUrl: "https://apinebula.com/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "third_party",
    icon: "apinebula",
  },
  {
    name: "AICodeMirror",
    websiteUrl: "https://www.aicodemirror.com",
    apiKeyUrl: "https://www.aicodemirror.com/register?invitecode=9915W3",
    settingsConfig: {
      baseUrl: "https://api.aicodemirror.com/api/codex/backend-api/codex",
      apiKey: "",
      model: "gpt-5.5",
    },
    icon: "aicodemirror",
    iconColor: "#000000",
  },
  {
    name: "PatewayAI",
    websiteUrl: "https://pateway.ai",
    apiKeyUrl: "https://pateway.ai/?ch=etzpm8&aff=WB6M6F67#/",
    settingsConfig: {
      baseUrl: "https://api.pateway.ai/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "third_party",
    icon: "pateway",
  },
  {
    name: "FennoAI",
    websiteUrl: "https://api.fenno.ai",
    apiKeyUrl:
      "https://api.fenno.ai/register?redirect=/purchase?tab=subscription%26group=16&aff=P9MR3D3PLCNL",
    settingsConfig: {
      baseUrl: "https://api.fenno.ai",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "fenno",
  },
  {
    name: "RunAPI",
    websiteUrl: "https://runapi.co",
    apiKeyUrl: "https://runapi.co/register?aff=iOKB",
    settingsConfig: {
      baseUrl: "https://runapi.co/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "runapi",
  },
  {
    name: "Unity2.ai",
    websiteUrl: "https://unity2.ai",
    apiKeyUrl: "https://unity2.ai/register?source=ccs",
    settingsConfig: {
      baseUrl: "https://api.unity2.ai",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "unity2",
  },
  {
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    settingsConfig: {
      baseUrl: "https://router.shengsuanyun.com/api/v1",
      apiKey: "",
      model: "openai/gpt-5.5",
    },
    category: "aggregator",
    icon: "shengsuanyun",
  },
  {
    name: "AIGoCode",
    websiteUrl: "https://aigocode.com",
    apiKeyUrl: "https://aigocode.com/invite/CC-SWITCH",
    settingsConfig: {
      baseUrl: "https://api.aigocode.com",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "third_party",
    icon: "aigocode",
    iconColor: "#5B7FFF",
  },
  {
    name: "SubRouter",
    websiteUrl: "https://subrouter.ai",
    apiKeyUrl: "https://subrouter.ai/register?aff=l3ri",
    settingsConfig: {
      baseUrl: "https://subrouter.ai/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "subrouter",
  },
  {
    name: "APIKEY.FUN",
    websiteUrl: "https://apikey.fun",
    apiKeyUrl: "https://apikey.fun/register?aff=CCSwitch",
    settingsConfig: {
      baseUrl: "https://api.apikey.fun/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "third_party",
    icon: "apikeyfun",
  },
  {
    name: "Code0",
    websiteUrl: "https://code0.ai",
    apiKeyUrl: "https://code0.ai/agent/register/B2XHxGjGmRvqgznY",
    settingsConfig: {
      baseUrl: "https://code0.ai/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "code0",
  },
  {
    name: "TeamoRouter",
    websiteUrl: "https://teamorouter.com",
    apiKeyUrl:
      "https://teamorouter.com/?utm_source=cc_switch&utm_medium=referral&utm_campaign=ai_directory",
    settingsConfig: {
      baseUrl: "https://api.teamorouter.com/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "teamorouter",
  },
  {
    name: "ClaudeCN",
    websiteUrl: "https://claudecn.top",
    apiKeyUrl: "https://claudecn.ai/register?aff=HEL9",
    settingsConfig: {
      baseUrl: "https://claudecn.top/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "third_party",
    icon: "claudecn",
  },
  {
    name: "火山Agentplan",
    websiteUrl:
      "https://www.volcengine.com/activity/codingplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://www.volcengine.com/activity/codingplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    settingsConfig: {
      baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
      apiKey: "",
      model: "ark-code-latest",
    },
    category: "cn_official",
    icon: "huoshan",
    iconColor: "#3370FF",
    modelCatalog: [
      {
        model: "ark-code-latest",
        displayName: "Ark Code Latest",
        contextWindow: 256000,
      },
    ],
  },
  {
    name: "BytePlus",
    websiteUrl:
      "https://www.byteplus.com/en/product/modelark?utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://www.byteplus.com/en/product/modelark?utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    settingsConfig: {
      baseUrl: "https://ark.ap-southeast.bytepluses.com/api/coding/v3",
      apiKey: "",
      model: "ark-code-latest",
    },
    category: "cn_official",
    icon: "byteplus",
    iconColor: "#3370FF",
    modelCatalog: [
      {
        model: "ark-code-latest",
        displayName: "Ark Code Latest",
        contextWindow: 256000,
      },
    ],
  },
  {
    name: "DouBaoSeed",
    websiteUrl:
      "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey?apikey=%7B%7D&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey?apikey=%7B%7D&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    settingsConfig: {
      baseUrl: "https://ark.cn-beijing.volces.com/api/v3",
      apiKey: "",
      model: "doubao-seed-2-1-pro-260628",
    },
    category: "cn_official",
    icon: "doubao",
    iconColor: "#3370FF",
    modelCatalog: [
      {
        model: "doubao-seed-2-1-pro-260628",
        displayName: "Doubao Seed 2.1 Pro",
        contextWindow: 262144,
      },
    ],
  },
  {
    name: "SiliconFlow",
    websiteUrl: "https://siliconflow.cn",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/YflgU2Ve",
    settingsConfig: {
      baseUrl: "https://api.siliconflow.cn/v1",
      apiKey: "",
      model: "Pro/MiniMaxAI/MiniMax-M2.7",
    },
    category: "aggregator",
    icon: "siliconflow",
    iconColor: "#6E29F6",
    modelCatalog: [
      {
        model: "Pro/MiniMaxAI/MiniMax-M2.7",
        displayName: "Pro / MiniMax M2.7",
        contextWindow: 200000,
      },
    ],
  },
  {
    name: "SiliconFlow en",
    websiteUrl: "https://siliconflow.com",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/YflgU2Ve",
    settingsConfig: {
      baseUrl: "https://api.siliconflow.com/v1",
      apiKey: "",
      model: "MiniMaxAI/MiniMax-M2.7",
    },
    category: "aggregator",
    icon: "siliconflow",
    iconColor: "#000000",
    modelCatalog: [
      {
        model: "MiniMaxAI/MiniMax-M2.7",
        displayName: "MiniMax M2.7",
        contextWindow: 200000,
      },
    ],
  },
  {
    name: "NekoCode",
    websiteUrl: "https://nekocode.ai",
    apiKeyUrl: "https://nekocode.ai?aff=CCSWITCH",
    settingsConfig: {
      baseUrl: "https://nekocode.ai/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "nekocode",
  },
  {
    name: "AtlasCloud",
    websiteUrl: "https://www.atlascloud.ai/console/coding-plan",
    apiKeyUrl: "https://www.atlascloud.ai/console/coding-plan",
    settingsConfig: {
      baseUrl: "https://api.atlascloud.ai/v1",
      apiKey: "",
      model: "zai-org/glm-5.1",
    },
    category: "aggregator",
    icon: "atlascloud",
    modelCatalog: [
      {
        model: "zai-org/glm-5.1",
        displayName: "GLM 5.1",
        contextWindow: 200000,
      },
    ],
  },
  {
    name: "Compshare",
    nameKey: "providerForm.presets.ucloud",
    websiteUrl: "https://www.compshare.cn",
    apiKeyUrl:
      "https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch",
    settingsConfig: {
      baseUrl: "https://api.modelverse.cn/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "ucloud",
    iconColor: "#000000",
  },
  {
    name: "Compshare Coding Plan",
    nameKey: "providerForm.presets.ucloudCoding",
    websiteUrl: "https://www.compshare.cn",
    apiKeyUrl:
      "https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch",
    settingsConfig: {
      baseUrl: "https://cp.compshare.cn/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "ucloud",
    iconColor: "#000000",
  },
  {
    name: "CCSub",
    websiteUrl: "https://www.ccsub.net",
    apiKeyUrl: "https://www.ccsub.net/register?ref=Y6Z8DXEA",
    settingsConfig: {
      baseUrl: "https://www.ccsub.net/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "ccsub",
  },
  {
    name: "SSSAiCode",
    websiteUrl: "https://sssaicodeapi.com",
    apiKeyUrl: "https://sssaicodeapi.com/register?ref=DCP0SM",
    settingsConfig: {
      baseUrl: "https://node-hk.sssaicodeapi.com/api/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "third_party",
    icon: "sssaicode",
    iconColor: "#000000",
  },
  {
    name: "Micu",
    websiteUrl: "https://www.micuapi.ai",
    apiKeyUrl: "https://www.micuapi.ai/register?aff=aOYQ",
    settingsConfig: {
      baseUrl: "https://www.micuapi.ai/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "third_party",
    icon: "micu",
    iconColor: "#000000",
  },
  {
    name: "RightCode",
    websiteUrl: "https://www.right.codes",
    apiKeyUrl: "https://www.right.codes/register?aff=CCSWITCH",
    settingsConfig: {
      baseUrl: "https://right.codes/codex/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "third_party",
    icon: "rc",
    iconColor: "#E96B2C",
  },
  {
    name: "ETok.ai",
    websiteUrl: "https://etok.ai",
    apiKeyUrl: "https://etok.ai",
    settingsConfig: {
      baseUrl: "https://api.etok.ai/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "third_party",
    icon: "etok",
    iconColor: "#000000",
  },
  {
    name: "Cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    settingsConfig: {
      baseUrl: "https://api.cubence.com/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "third_party",
    icon: "cubence",
    iconColor: "#000000",
  },
  {
    name: "CrazyRouter",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    settingsConfig: {
      baseUrl: "https://cn.crazyrouter.com/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    icon: "crazyrouter",
    iconColor: "#000000",
  },
  {
    name: "DMXAPI",
    websiteUrl: "https://www.dmxapi.cn",
    settingsConfig: {
      baseUrl: "https://www.dmxapi.cn/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "dmxapi",
  },
  {
    name: "Qiniu",
    nameKey: "providerForm.presets.qiniu",
    websiteUrl: "https://s.qiniu.com/nMvAvy",
    apiKeyUrl: "https://s.qiniu.com/nMvAvy",
    settingsConfig: {
      baseUrl: "https://api.qnaigc.com/bypass/openai/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "qiniu",
  },
  {
    name: "SudoCode.chat",
    websiteUrl: "https://sudocode.chat",
    apiKeyUrl:
      "https://sudocode.chat/register?utm_source=ccswitch&utm_medium=partner",
    settingsConfig: {
      baseUrl: "https://api.sudocode.chat/v1",
      apiKey: "",
      model: "gpt-5.6-sol",
    },
    category: "third_party",
    icon: "sudocode",
  },
  {
    name: "SudoCode.us",
    websiteUrl: "https://sudocode.us",
    apiKeyUrl: "https://sudocode.us",
    settingsConfig: {
      baseUrl: "https://sudocode.us/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "third_party",
    icon: "sudocode-us",
  },
  {
    name: "Amux",
    websiteUrl: "https://amux.ai",
    apiKeyUrl: "https://amux.ai",
    settingsConfig: {
      baseUrl: "https://api.amux.ai/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "amux",
  },
  {
    name: "DeepSeek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    settingsConfig: {
      baseUrl: "https://api.deepseek.com",
      apiKey: "",
      model: "deepseek-v4-flash",
    },
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#1E88E5",
    modelCatalog: [
      {
        model: "deepseek-v4-flash",
        displayName: "DeepSeek V4 Flash",
        contextWindow: 1000000,
      },
      {
        model: "deepseek-v4-pro",
        displayName: "DeepSeek V4 Pro",
        contextWindow: 1000000,
      },
    ],
  },
  {
    name: "Zhipu GLM",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    settingsConfig: {
      baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4",
      apiKey: "",
      model: "glm-5.2",
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
    modelCatalog: [
      { model: "glm-5.2", displayName: "GLM-5.2", contextWindow: 200000 },
    ],
  },
  {
    name: "Zhipu GLM en",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe?ic=8JVLJQFSKB",
    settingsConfig: {
      baseUrl: "https://api.z.ai/api/coding/paas/v4",
      apiKey: "",
      model: "glm-5.2",
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
    modelCatalog: [
      { model: "glm-5.2", displayName: "GLM-5.2", contextWindow: 200000 },
    ],
  },
  {
    name: "Baidu Qianfan Coding Plan",
    websiteUrl: "https://cloud.baidu.com/product/qianfan_modelbuilder",
    apiKeyUrl:
      "https://console.bce.baidu.com/qianfan/ais/console/applicationConsole/application",
    settingsConfig: {
      baseUrl: "https://qianfan.baidubce.com/v2/coding",
      apiKey: "",
      model: "qianfan-code-latest",
    },
    category: "cn_official",
    icon: "baidu",
    iconColor: "#2932E1",
    modelCatalog: [
      {
        model: "qianfan-code-latest",
        displayName: "Qianfan Code Latest",
        contextWindow: 131072,
      },
    ],
  },
  {
    name: "Bailian",
    websiteUrl: "https://bailian.console.aliyun.com",
    apiKeyUrl: "https://bailian.console.aliyun.com/#/api-key",
    settingsConfig: {
      baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      apiKey: "",
      model: "qwen3-coder-plus",
    },
    category: "cn_official",
    icon: "bailian",
    iconColor: "#624AFF",
    modelCatalog: [
      {
        model: "qwen3-coder-plus",
        displayName: "Qwen3 Coder Plus",
        contextWindow: 1048576,
      },
    ],
  },
  {
    name: "StepFun",
    websiteUrl: "https://platform.stepfun.com/step-plan",
    apiKeyUrl: "https://platform.stepfun.com/interface-key",
    settingsConfig: {
      baseUrl: "https://api.stepfun.com/step_plan/v1",
      apiKey: "",
      model: "step-3.7-flash",
    },
    category: "cn_official",
    icon: "stepfun",
    iconColor: "#16D6D2",
    modelCatalog: [
      {
        model: "step-3.7-flash",
        displayName: "Step 3.7 Flash",
        contextWindow: 262144,
      },
      {
        model: "step-3.5-flash-2603",
        displayName: "Step 3.5 Flash 2603",
        contextWindow: 262144,
      },
      {
        model: "step-3.5-flash",
        displayName: "Step 3.5 Flash",
        contextWindow: 262144,
      },
    ],
  },
  {
    name: "StepFun en",
    websiteUrl: "https://platform.stepfun.ai/step-plan",
    apiKeyUrl: "https://platform.stepfun.ai/interface-key",
    settingsConfig: {
      baseUrl: "https://api.stepfun.ai/step_plan/v1",
      apiKey: "",
      model: "step-3.7-flash",
    },
    category: "cn_official",
    icon: "stepfun",
    iconColor: "#16D6D2",
    modelCatalog: [
      {
        model: "step-3.7-flash",
        displayName: "Step 3.7 Flash",
        contextWindow: 262144,
      },
      {
        model: "step-3.5-flash-2603",
        displayName: "Step 3.5 Flash 2603",
        contextWindow: 262144,
      },
      {
        model: "step-3.5-flash",
        displayName: "Step 3.5 Flash",
        contextWindow: 262144,
      },
    ],
  },
  {
    name: "ModelScope",
    websiteUrl: "https://modelscope.cn",
    apiKeyUrl: "https://modelscope.cn/my/myaccesstoken",
    settingsConfig: {
      baseUrl: "https://api-inference.modelscope.cn/v1",
      apiKey: "",
      model: "ZhipuAI/GLM-5.1",
    },
    category: "aggregator",
    icon: "modelscope",
    iconColor: "#624AFF",
    modelCatalog: [
      {
        model: "ZhipuAI/GLM-5.1",
        displayName: "ZhipuAI / GLM-5.1",
        contextWindow: 200000,
      },
    ],
  },
  {
    name: "Longcat",
    websiteUrl: "https://longcat.chat/platform",
    apiKeyUrl: "https://longcat.chat/platform/api_keys",
    settingsConfig: {
      baseUrl: "https://api.longcat.chat/openai/v1",
      apiKey: "",
      model: "LongCat-2.0",
    },
    category: "cn_official",
    icon: "longcat",
    iconColor: "#29E154",
    modelCatalog: [
      {
        model: "LongCat-2.0",
        displayName: "LongCat 2.0",
        contextWindow: 1048576,
      },
    ],
  },
  {
    name: "MiniMax",
    websiteUrl: "https://platform.minimaxi.com",
    apiKeyUrl: "https://platform.minimaxi.com/subscribe/coding-plan",
    settingsConfig: {
      baseUrl: "https://api.minimaxi.com/v1",
      apiKey: "",
      model: "MiniMax-M3",
    },
    category: "cn_official",
    icon: "minimax",
    iconColor: "#FF6B6B",
    modelCatalog: [
      {
        model: "MiniMax-M3",
        displayName: "MiniMax-M3",
        contextWindow: 1000000,
      },
    ],
  },
  {
    name: "MiniMax en",
    websiteUrl: "https://platform.minimax.io",
    apiKeyUrl: "https://platform.minimax.io/subscribe/coding-plan",
    settingsConfig: {
      baseUrl: "https://api.minimax.io/v1",
      apiKey: "",
      model: "MiniMax-M3",
    },
    category: "cn_official",
    icon: "minimax",
    iconColor: "#FF6B6B",
    modelCatalog: [
      {
        model: "MiniMax-M3",
        displayName: "MiniMax-M3",
        contextWindow: 1000000,
      },
    ],
  },
  {
    name: "BaiLing",
    websiteUrl: "https://alipaytbox.yuque.com/sxs0ba/ling/get_started",
    apiKeyUrl: "https://ling.tbox.cn/open",
    settingsConfig: {
      baseUrl: "https://api.tbox.cn/api/llm/v1",
      apiKey: "",
      model: "Ling-2.6-1T",
    },
    category: "cn_official",
    modelCatalog: [
      {
        model: "Ling-2.6-1T",
        displayName: "Ling-2.6-1T",
        contextWindow: 262144,
      },
    ],
  },
  {
    name: "Xiaomi MiMo",
    websiteUrl: "https://platform.xiaomimimo.com",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/api-keys",
    settingsConfig: {
      baseUrl: "https://api.xiaomimimo.com/v1",
      apiKey: "",
      model: "mimo-v2.5-pro",
    },
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
    modelCatalog: [
      {
        model: "mimo-v2.5-pro",
        displayName: "MiMo V2.5 Pro",
        contextWindow: 1048576,
      },
      { model: "mimo-v2.5", displayName: "MiMo V2.5", contextWindow: 1048576 },
    ],
  },
  {
    name: "Xiaomi MiMo Token Plan (China)",
    websiteUrl: "https://platform.xiaomimimo.com/#/token-plan",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/plan-manage",
    settingsConfig: {
      baseUrl: "https://token-plan-cn.xiaomimimo.com/v1",
      apiKey: "",
      model: "mimo-v2.5-pro",
    },
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
    modelCatalog: [
      {
        model: "mimo-v2.5-pro",
        displayName: "MiMo V2.5 Pro",
        contextWindow: 1048576,
      },
      { model: "mimo-v2.5", displayName: "MiMo V2.5", contextWindow: 1048576 },
    ],
  },
  {
    name: "Novita AI",
    websiteUrl: "https://novita.ai",
    apiKeyUrl: "https://novita.ai",
    settingsConfig: {
      baseUrl: "https://api.novita.ai/openai/v1",
      apiKey: "",
      model: "zai-org/glm-5.1",
    },
    category: "aggregator",
    icon: "novita",
    iconColor: "#000000",
    modelCatalog: [
      {
        model: "zai-org/glm-5.1",
        displayName: "GLM-5.1",
        contextWindow: 202800,
      },
    ],
  },
  {
    name: "xAI (Grok)",
    websiteUrl: "https://x.ai/api",
    apiKeyUrl: "https://console.x.ai",
    settingsConfig: {
      baseUrl: "https://api.x.ai/v1",
      apiKey: "",
      model: "grok-4.5",
    },
    category: "third_party",
    icon: "xai",
    iconColor: "#000000",
    modelCatalog: [
      { model: "grok-4.5", displayName: "Grok 4.5", contextWindow: 500000 },
    ],
  },
  {
    name: "Nvidia",
    websiteUrl: "https://build.nvidia.com",
    apiKeyUrl: "https://build.nvidia.com/settings/api-keys",
    settingsConfig: {
      baseUrl: "https://integrate.api.nvidia.com/v1",
      apiKey: "",
      model: "moonshotai/kimi-k2.5",
    },
    category: "aggregator",
    icon: "nvidia",
    iconColor: "#000000",
    modelCatalog: [
      {
        model: "moonshotai/kimi-k2.5",
        displayName: "Kimi K2.5",
        contextWindow: 262144,
      },
    ],
  },
  {
    name: "OpenCode Go",
    websiteUrl: "https://opencode.ai/go",
    apiKeyUrl: "https://opencode.ai/go?ref=2YTRG2NGTX",
    settingsConfig: {
      baseUrl: "https://opencode.ai/zen/go/v1",
      apiKey: "",
      model: "glm-5.2",
    },
    category: "third_party",
    icon: "opencode",
    iconColor: "#211E1E",
    modelCatalog: [
      { model: "glm-5.2", displayName: "GLM 5.2", contextWindow: 204800 },
      { model: "glm-5.1", displayName: "GLM 5.1", contextWindow: 204800 },
      {
        model: "kimi-k2.7-code",
        displayName: "Kimi K2.7 Code",
        contextWindow: 262144,
      },
      { model: "deepseek-v4-pro", displayName: "DeepSeek V4 Pro" },
      { model: "deepseek-v4-flash", displayName: "DeepSeek V4 Flash" },
      {
        model: "mimo-v2.5-pro",
        displayName: "MiMo V2.5 Pro",
        contextWindow: 1048576,
      },
    ],
  },
  {
    name: "AiHubMix",
    websiteUrl: "https://aihubmix.com",
    settingsConfig: {
      baseUrl: "https://aihubmix.com/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "aihubmix",
    iconColor: "#006FFB",
  },
  {
    name: "CherryIN",
    websiteUrl: "https://open.cherryin.ai",
    apiKeyUrl: "https://open.cherryin.ai/console/token",
    settingsConfig: {
      baseUrl: "https://open.cherryin.net/v1",
      apiKey: "",
      model: "openai/gpt-5.5",
    },
    category: "aggregator",
    icon: "cherryin",
  },
  {
    name: "RelaxyCode",
    websiteUrl: "https://www.relaxycode.com",
    apiKeyUrl: "https://www.relaxycode.com/register",
    settingsConfig: {
      baseUrl: "https://www.relaxycode.com/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "third_party",
    icon: "relaxcode",
  },
  {
    name: "E-FlowCode",
    websiteUrl: "https://e-flowcode.cc",
    apiKeyUrl: "https://e-flowcode.cc",
    settingsConfig: {
      baseUrl: "https://e-flowcode.cc/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "third_party",
    icon: "eflowcode",
    iconColor: "#000000",
  },
  {
    name: "PIPELLM",
    websiteUrl: "https://code.pipellm.ai",
    apiKeyUrl: "https://code.pipellm.ai/login?ref=uvw650za",
    settingsConfig: {
      baseUrl: "https://cc-api.pipellm.ai/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "pipellm",
  },
  {
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      baseUrl: "https://openrouter.ai/api/v1",
      apiKey: "",
      model: "gpt-5.5",
    },
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6566F1",
  },
  {
    name: "TheRouter",
    websiteUrl: "https://therouter.ai",
    apiKeyUrl: "https://dashboard.therouter.ai",
    settingsConfig: {
      baseUrl: "https://api.therouter.ai/v1",
      apiKey: "",
      model: "openai/gpt-5.3-codex",
    },
    category: "aggregator",
  },
];
