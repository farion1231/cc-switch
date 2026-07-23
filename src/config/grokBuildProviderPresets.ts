/**
 * Grok Build (Grok CLI) 预设供应商配置模板
 *
 * 独立维护，与 codexProviderPresets.ts 无数据联动（Jason 2026-07-21 定）。
 * 初始条目取自当时的 Codex 预设快照，此后两边各自演进：
 * 合作伙伴链接 / 图标 / endpoint 变更需要在本文件单独修改。
 *
 * 收录规则：
 * - 不含官方 / 托管 OAuth 预设：Grok CLI 自带 xAI 订阅登录，官方态走
 *   独立的 "Grok Official" 条目（对应 providers_seed.rs 的 seed，
 *   空 config = 不写自定义模型表）。
 * - 不含国产模型官方直连（cn_official）与纯开源模型托管站
 *   （SiliconFlow / ModelScope / Novita / Nvidia / AtlasCloud / OpenCode Go）：
 *   这些上游没有 Grok 模型，无法在 Grok CLI 中使用。
 * - 只收聚合站与第三方中转站，默认模型统一为 grok-4.5；
 *   OpenRouter 系命名空间的路由站用 "x-ai/grok-4.5"。
 *
 * config 字段沿用 Codex 风格 TOML 作为载体：Grok 表单只从中提取
 * base_url / model / wire_api 三个字段（extractCodex* 工具），再重建
 * Grok CLI 自己的 config.toml。
 */
import type { ProviderCategory } from "../types";
import type { CodexApiFormat } from "../types";
import { GROK_BUILD_DEFAULT_MODEL } from "../utils/grokBuildConfig";

export interface GrokBuildProviderPreset {
  name: string;
  nameKey?: string; // i18n key for localized display name
  websiteUrl: string;
  apiKeyUrl?: string;
  auth: Record<string, any>;
  config: string; // Codex 风格 TOML 载体（只消费 base_url / model / wire_api）
  isOfficial?: boolean;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  endpointCandidates?: string[];
  icon?: string;
  iconColor?: string;
  apiFormat?: CodexApiFormat;
}

// 官方条目与后端 seed（providers_seed.rs 的 "Grok Official"）对应：
// 空 config = 不写自定义模型表，Grok CLI 回落到自带的 xAI OAuth 登录。
// 预设 id 复用固定 provider id，AddProviderDialog 据此走 ensure seed 流程。
export const grokBuildOfficialPreset: GrokBuildProviderPreset = {
  name: "Grok Official",
  websiteUrl: "https://x.ai/grok",
  isOfficial: true,
  category: "official",
  auth: {},
  config: "",
  icon: "grok",
  iconColor: "currentColor",
};

/** OpenRouter 系命名空间路由站的 Grok 模型 id */
const OPENROUTER_STYLE_GROK_MODEL = "x-ai/grok-4.5";

const grokAuth = (): Record<string, any> => ({ OPENAI_API_KEY: "" });

function grokPresetConfig(
  providerName: string,
  baseUrl: string,
  model = GROK_BUILD_DEFAULT_MODEL,
): string {
  const tomlString = (value: string) => JSON.stringify(value);

  return `model_provider = "custom"
model = ${tomlString(model)}

[model_providers.custom]
name = ${tomlString(providerName)}
base_url = ${tomlString(baseUrl)}
wire_api = "responses"
requires_openai_auth = true`;
}

export const grokBuildProviderPresets: GrokBuildProviderPreset[] = [
  // ===== 赞助商预设：文件顺序 = 应用内展示顺序，与 README 赞助商表对齐 =====
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.com",
    apiKeyUrl: "https://www.packyapi.com/register?aff=cc-switch",
    auth: grokAuth(),
    config: grokPresetConfig("PackyCode", "https://www.packyapi.com/v1"),
    endpointCandidates: [
      "https://www.packyapi.com/v1",
      "https://api-slb.packyapi.com/v1",
    ],
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "packycode",
    icon: "packycode",
  },
  {
    name: "ZetaAPI",
    websiteUrl: "https://zetaapi.ai",
    apiKeyUrl: "https://zetaapi.ai/go/u117",
    auth: grokAuth(),
    config: grokPresetConfig("ZetaAPI", "https://api.zetaapi.ai/v1"),
    endpointCandidates: ["https://api.zetaapi.ai/v1"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "zetaapi",
    icon: "zetaapi",
  },
  {
    name: "APINebula",
    websiteUrl: "https://apinebula.com",
    apiKeyUrl: "https://apinebula.com/VjM74M",
    auth: grokAuth(),
    config: grokPresetConfig("APINebula", "https://apinebula.com/v1"),
    endpointCandidates: ["https://apinebula.com/v1"],
    apiFormat: "openai_responses",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apinebula",
    icon: "apinebula",
  },
  {
    name: "AICodeMirror",
    websiteUrl: "https://www.aicodemirror.com",
    apiKeyUrl: "https://www.aicodemirror.com/register?invitecode=9915W3",
    auth: grokAuth(),
    config: grokPresetConfig(
      "AICodeMirror",
      "https://api.aicodemirror.com/api/codex/backend-api/codex",
    ),
    endpointCandidates: [
      "https://api.aicodemirror.com/api/codex/backend-api/codex",
      "https://api.claudecode.net.cn/api/codex/backend-api/codex",
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
    auth: grokAuth(),
    config: grokPresetConfig("PatewayAI", "https://api.pateway.ai/v1"),
    endpointCandidates: ["https://api.pateway.ai/v1"],
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "patewayai",
    icon: "pateway",
  },
  {
    name: "FennoAI",
    websiteUrl: "https://api.fenno.ai",
    apiKeyUrl:
      "https://api.fenno.ai/register?redirect=/purchase?tab=subscription%26group=16&aff=P9MR3D3PLCNL",
    auth: grokAuth(),
    config: grokPresetConfig("FennoAI", "https://api.fenno.ai"),
    endpointCandidates: ["https://api.fenno.ai"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "fenno",
    icon: "fenno",
  },
  {
    name: "RunAPI",
    websiteUrl: "https://runapi.co",
    apiKeyUrl: "https://runapi.co/register?aff=iOKB",
    auth: grokAuth(),
    config: grokPresetConfig("RunAPI", "https://runapi.co/v1"),
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "runapi",
    icon: "runapi",
  },
  {
    name: "Unity2.ai",
    websiteUrl: "https://unity2.ai",
    apiKeyUrl: "https://unity2.ai/register?source=ccs",
    auth: grokAuth(),
    config: grokPresetConfig("Unity2.ai", "https://api.unity2.ai"),
    endpointCandidates: ["https://api.unity2.ai"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "unity2",
    icon: "unity2",
  },
  {
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    auth: grokAuth(),
    config: grokPresetConfig(
      "Shengsuanyun",
      "https://router.shengsuanyun.com/api/v1",
      OPENROUTER_STYLE_GROK_MODEL,
    ),
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "shengsuanyun",
    icon: "shengsuanyun",
  },
  {
    name: "AIGoCode",
    websiteUrl: "https://aigocode.com",
    apiKeyUrl: "https://aigocode.com/invite/CC-SWITCH",
    auth: grokAuth(),
    config: grokPresetConfig("AIGoCode", "https://api.aigocode.com"),
    endpointCandidates: ["https://api.aigocode.com"],
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aigocode",
    icon: "aigocode",
    iconColor: "#5B7FFF",
  },
  {
    name: "SubRouter",
    websiteUrl: "https://subrouter.ai",
    apiKeyUrl: "https://subrouter.ai/register?aff=l3ri",
    auth: grokAuth(),
    config: grokPresetConfig("SubRouter", "https://subrouter.ai/v1"),
    endpointCandidates: ["https://subrouter.ai/v1"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "subrouter",
    icon: "subrouter",
  },
  {
    name: "APIKEY.FUN",
    websiteUrl: "https://apikey.fun",
    apiKeyUrl: "https://apikey.fun/register?aff=CCSwitch",
    auth: grokAuth(),
    config: grokPresetConfig("APIKEY.FUN", "https://api.apikey.fun/v1"),
    endpointCandidates: [
      "https://api.apikey.fun/v1",
      "https://slb.apikey.fun/v1",
    ],
    apiFormat: "openai_responses",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apikeyfun",
    icon: "apikeyfun",
  },
  {
    name: "Code0",
    websiteUrl: "https://code0.ai",
    apiKeyUrl: "https://code0.ai/agent/register/B2XHxGjGmRvqgznY",
    auth: grokAuth(),
    config: grokPresetConfig("Code0", "https://code0.ai/v1"),
    endpointCandidates: ["https://code0.ai/v1"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "code0",
    icon: "code0",
  },
  {
    name: "TeamoRouter",
    websiteUrl: "https://teamorouter.com",
    apiKeyUrl:
      "https://teamorouter.com/?utm_source=cc_switch&utm_medium=referral&utm_campaign=ai_directory",
    auth: grokAuth(),
    config: grokPresetConfig("TeamoRouter", "https://api.teamorouter.com/v1"),
    endpointCandidates: ["https://api.teamorouter.com/v1"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "teamorouter",
    icon: "teamorouter",
  },
  {
    name: "ClaudeCN",
    websiteUrl: "https://claudecn.top",
    apiKeyUrl: "https://claudecn.ai/register?aff=HEL9",
    auth: grokAuth(),
    config: grokPresetConfig("ClaudeCN", "https://claudecn.top/v1"),
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "claudecn",
    icon: "claudecn",
  },
  {
    name: "NekoCode",
    websiteUrl: "https://nekocode.ai",
    apiKeyUrl: "https://nekocode.ai?aff=CCSWITCH",
    auth: grokAuth(),
    config: grokPresetConfig("NekoCode", "https://nekocode.ai/v1"),
    endpointCandidates: ["https://nekocode.ai/v1"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "nekocode",
    icon: "nekocode",
  },
  {
    name: "Compshare",
    nameKey: "providerForm.presets.ucloud",
    websiteUrl: "https://www.compshare.cn",
    apiKeyUrl:
      "https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch",
    auth: grokAuth(),
    config: grokPresetConfig("Compshare", "https://api.modelverse.cn/v1"),
    endpointCandidates: ["https://api.modelverse.cn/v1"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ucloud",
    icon: "ucloud",
    iconColor: "#000000",
  },
  {
    name: "Compshare Coding Plan",
    nameKey: "providerForm.presets.ucloudCoding",
    websiteUrl: "https://www.compshare.cn",
    apiKeyUrl:
      "https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch",
    auth: grokAuth(),
    config: grokPresetConfig(
      "Compshare Coding Plan",
      "https://cp.compshare.cn/v1",
    ),
    endpointCandidates: ["https://cp.compshare.cn/v1"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ucloud",
    icon: "ucloud",
    iconColor: "#000000",
  },
  {
    name: "CCSub",
    websiteUrl: "https://www.ccsub.net",
    apiKeyUrl: "https://www.ccsub.net/register?ref=Y6Z8DXEA",
    auth: grokAuth(),
    config: grokPresetConfig("CCSub", "https://www.ccsub.net/v1"),
    endpointCandidates: ["https://www.ccsub.net/v1"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ccsub",
    icon: "ccsub",
  },
  {
    name: "SSSAiCode",
    websiteUrl: "https://sssaicodeapi.com",
    apiKeyUrl: "https://sssaicodeapi.com/register?ref=DCP0SM",
    auth: grokAuth(),
    config: grokPresetConfig(
      "SSSAiCode",
      "https://node-hk.sssaicodeapi.com/api/v1",
    ),
    endpointCandidates: [
      "https://node-hk.sssaicodeapi.com/api/v1",
      "https://node-hk.sssaiapi.com/api/v1",
      "https://node-cf.sssaicodeapi.com/api/v1",
    ],
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "sssaicode",
    icon: "sssaicode",
    iconColor: "#000000",
  },
  {
    name: "Micu",
    websiteUrl: "https://www.micuapi.ai",
    apiKeyUrl: "https://www.micuapi.ai/register?aff=aOYQ",
    auth: grokAuth(),
    config: grokPresetConfig("Micu", "https://www.micuapi.ai/v1"),
    endpointCandidates: ["https://www.micuapi.ai/v1"],
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "micu",
    icon: "micu",
    iconColor: "#000000",
  },
  {
    name: "RightCode",
    websiteUrl: "https://www.right.codes",
    apiKeyUrl: "https://www.right.codes/register?aff=CCSWITCH",
    auth: grokAuth(),
    config: grokPresetConfig("RightCode", "https://right.codes/codex/v1"),
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
    auth: grokAuth(),
    config: grokPresetConfig("ETok.ai", "https://api.etok.ai/v1"),
    endpointCandidates: ["https://api.etok.ai/v1"],
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "etok",
    icon: "etok",
    iconColor: "#000000",
  },
  {
    name: "Cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    auth: grokAuth(),
    config: grokPresetConfig("Cubence", "https://api.cubence.com/v1"),
    endpointCandidates: [
      "https://api.cubence.com/v1",
      "https://api-cf.cubence.com/v1",
      "https://api-dmit.cubence.com/v1",
      "https://api-bwg.cubence.com/v1",
    ],
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "cubence",
    icon: "cubence",
    iconColor: "#000000",
  },
  {
    name: "CrazyRouter",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    auth: grokAuth(),
    config: grokPresetConfig("CrazyRouter", "https://cn.crazyrouter.com/v1"),
    endpointCandidates: ["https://cn.crazyrouter.com/v1"],
    isPartner: true,
    partnerPromotionKey: "crazyrouter",
    icon: "crazyrouter",
    iconColor: "#000000",
  },
  {
    name: "DMXAPI",
    websiteUrl: "https://www.dmxapi.cn",
    auth: grokAuth(),
    config: grokPresetConfig("DMXAPI", "https://www.dmxapi.cn/v1"),
    endpointCandidates: ["https://www.dmxapi.cn/v1"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "dmxapi",
  },
  {
    name: "Qiniu",
    nameKey: "providerForm.presets.qiniu",
    websiteUrl: "https://s.qiniu.com/nMvAvy",
    apiKeyUrl: "https://s.qiniu.com/nMvAvy",
    auth: grokAuth(),
    config: grokPresetConfig(
      "Qiniu",
      "https://api.qnaigc.com/bypass/openai/v1",
    ),
    endpointCandidates: [
      "https://api.qnaigc.com/bypass/openai/v1",
      "https://api.modelink.ai/bypass/openai/v1",
    ],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "qiniu",
    icon: "qiniu",
  },
  {
    name: "SudoCode.chat",
    websiteUrl: "https://sudocode.chat",
    apiKeyUrl:
      "https://sudocode.chat/register?utm_source=ccswitch&utm_medium=partner",
    auth: grokAuth(),
    config: grokPresetConfig("SudoCode.chat", "https://api.sudocode.chat/v1"),
    endpointCandidates: ["https://api.sudocode.chat/v1"],
    apiFormat: "openai_responses",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "sudocode",
    icon: "sudocode",
  },
  {
    name: "SudoCode.us",
    websiteUrl: "https://sudocode.us",
    apiKeyUrl: "https://sudocode.us",
    auth: grokAuth(),
    config: grokPresetConfig("SudoCode.us", "https://sudocode.us/v1"),
    endpointCandidates: ["https://sudocode.us/v1", "https://sudocode.run/v1"],
    apiFormat: "openai_responses",
    category: "third_party",
    isPartner: true,
    icon: "sudocode-us",
  },
  // ===== 非赞助商预设：应用内展示按显示名排序，此处文件顺序不影响展示 =====
  {
    name: "xAI (Grok)",
    websiteUrl: "https://x.ai/api",
    apiKeyUrl: "https://console.x.ai",
    auth: grokAuth(),
    config: grokPresetConfig("xAI (Grok)", "https://api.x.ai/v1"),
    endpointCandidates: ["https://api.x.ai/v1"],
    apiFormat: "openai_responses",
    category: "third_party",
    icon: "xai",
    iconColor: "#000000",
  },
  {
    name: "Amux",
    websiteUrl: "https://amux.ai",
    apiKeyUrl: "https://amux.ai",
    auth: grokAuth(),
    config: grokPresetConfig("Amux", "https://api.amux.ai/v1"),
    endpointCandidates: ["https://api.amux.ai/v1"],
    category: "aggregator",
    icon: "amux",
  },
  {
    name: "AiHubMix",
    websiteUrl: "https://aihubmix.com",
    auth: grokAuth(),
    config: grokPresetConfig("AiHubMix", "https://aihubmix.com/v1"),
    endpointCandidates: [
      "https://aihubmix.com/v1",
      "https://api.aihubmix.com/v1",
    ],
    category: "aggregator",
    icon: "aihubmix",
    iconColor: "#006FFB",
  },
  {
    name: "CherryIN",
    websiteUrl: "https://open.cherryin.ai",
    apiKeyUrl: "https://open.cherryin.ai/console/token",
    auth: grokAuth(),
    config: grokPresetConfig(
      "CherryIN",
      "https://open.cherryin.net/v1",
      OPENROUTER_STYLE_GROK_MODEL,
    ),
    endpointCandidates: ["https://open.cherryin.net/v1"],
    category: "aggregator",
    icon: "cherryin",
  },
  {
    name: "RelaxyCode",
    websiteUrl: "https://www.relaxycode.com",
    apiKeyUrl: "https://www.relaxycode.com/register",
    auth: grokAuth(),
    config: grokPresetConfig("RelaxyCode", "https://www.relaxycode.com/v1"),
    category: "third_party",
    icon: "relaxcode",
  },
  {
    name: "E-FlowCode",
    websiteUrl: "https://e-flowcode.cc",
    apiKeyUrl: "https://e-flowcode.cc",
    auth: grokAuth(),
    config: grokPresetConfig("E-FlowCode", "https://e-flowcode.cc/v1"),
    endpointCandidates: ["https://e-flowcode.cc/v1"],
    category: "third_party",
    icon: "eflowcode",
    iconColor: "#000000",
  },
  {
    name: "PIPELLM",
    websiteUrl: "https://code.pipellm.ai",
    apiKeyUrl: "https://code.pipellm.ai/login?ref=uvw650za",
    auth: grokAuth(),
    config: grokPresetConfig("PIPELLM", "https://cc-api.pipellm.ai/v1"),
    endpointCandidates: ["https://cc-api.pipellm.ai/v1"],
    category: "aggregator",
    icon: "pipellm",
  },
  {
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    auth: grokAuth(),
    config: grokPresetConfig(
      "OpenRouter",
      "https://openrouter.ai/api/v1",
      OPENROUTER_STYLE_GROK_MODEL,
    ),
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6566F1",
  },
  {
    name: "TheRouter",
    websiteUrl: "https://therouter.ai",
    apiKeyUrl: "https://dashboard.therouter.ai",
    auth: grokAuth(),
    config: grokPresetConfig(
      "TheRouter",
      "https://api.therouter.ai/v1",
      OPENROUTER_STYLE_GROK_MODEL,
    ),
    endpointCandidates: ["https://api.therouter.ai/v1"],
    category: "aggregator",
  },
];
