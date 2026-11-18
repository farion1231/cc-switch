import type { ProviderCategory } from "@/types";
import type { PresetFamilyFields } from "@/config/presetFamilies";
import type { PresetTheme } from "@/config/claudeProviderPresets";

/**
 * AtomCode（共存型）供应商预设：settingsConfig 直接对应后端
 * `atomcode_config::validate_provider` 校验的形状 —— `account` 一个账号节点
 * 加 `models` 下的模型表；写入 config.toml 时后端会把 `provider_accounts.<id>`
 * 设为 account、把每个模型的 `account` 字段改成账号键，所以这里不用写它。
 *
 * `account.provider` 不是 wire 类型，而是 **AtomCode 预设 id**
 * （crates/atomcode-config/src/config/provider_preset.rs）：`openai-compatible` /
 * `anthropic-compatible` / `openai-responses` / `deepseek-messages` / `ollama`
 * 五个通用协议，加上厂商预设（`openai`、`anthropic`、`deepseek`、`zhipu`、
 * `moonshot`、`minimax`、`volcengine`、`opencode` …）。AtomCode 对未知 id 会
 * **静默回退到 openai-compatible**，所以写错值不会报错、只会换协议跑。
 */
export type AtomCodeProtocolId =
  | "openai-compatible"
  | "anthropic-compatible"
  | "openai-responses"
  | "deepseek-messages"
  | "ollama";

export type AtomCodeVendorPresetId =
  | "openai"
  | "anthropic"
  | "deepseek"
  | "zhipu"
  | "zhipu-coding"
  | "aliyun"
  | "siliconflow"
  | "moonshot"
  | "minimax"
  | "minimax-token-plan"
  | "xiaomi-mimo"
  | "longcat"
  | "volcengine"
  | "volcengine-coding"
  | "openrouter"
  | "taotoken"
  | "opencode";

/** 早期 cc-switch 写入的 wire 名（AtomCode 不认，后端写盘时会归一化）。 */
export type AtomCodeLegacyProviderId = "claude" | "openai_responses" | "gemini";

export type AtomCodeProviderType =
  | AtomCodeProtocolId
  | AtomCodeVendorPresetId
  | AtomCodeLegacyProviderId;

export interface AtomCodeModelEntry {
  model: string;
  context_window?: number;
}

export interface AtomCodeProviderPreset extends PresetFamilyFields {
  name: string;
  /** config.toml 里的账号键：字母、数字、`-`、`_`、`.`（后端 validate_provider 同款） */
  providerKey: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  category: ProviderCategory;
  icon?: string;
  iconColor?: string;
  /** i18n key for localized display name */
  nameKey?: string;
  theme?: PresetTheme;
  settingsConfig: {
    account: {
      provider: AtomCodeProviderType;
      base_url: string;
      api_key?: string;
    };
    models: Record<string, AtomCodeModelEntry>;
  };
}

/** 表单下拉的「协议」选项，与 AtomCode TUI 的 PROTOCOLS 一致。 */
export const ATOMCODE_PROVIDER_TYPES: {
  value: AtomCodeProtocolId;
  label: string;
}[] = [
  { value: "openai-compatible", label: "OpenAI Chat Completions" },
  { value: "anthropic-compatible", label: "Anthropic Messages" },
  { value: "openai-responses", label: "OpenAI Responses" },
  { value: "deepseek-messages", label: "DeepSeek Messages" },
  { value: "ollama", label: "Ollama (local)" },
];

/**
 * 厂商预设 id 的显示名（AtomCode registry 的 display_name）。预设在
 * `account.provider` 里写厂商 id 后，编辑该账号时下拉需要能显示出来。
 */
export const ATOMCODE_VENDOR_PROVIDER_LABELS: Record<string, string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  deepseek: "DeepSeek",
  "deepseek-messages": "DeepSeek (Messages API)",
  zhipu: "Zhipu AI",
  "zhipu-coding": "Zhipu AI Coding Plan",
  aliyun: "Alibaba Cloud Model Studio",
  siliconflow: "SiliconFlow",
  moonshot: "Moonshot AI",
  minimax: "MiniMax",
  "minimax-token-plan": "MiniMax Token Plan",
  "xiaomi-mimo": "Xiaomi MiMo",
  longcat: "LongCat",
  volcengine: "Volcengine Ark",
  "volcengine-coding": "Volcengine Ark Coding Plan",
  openrouter: "OpenRouter",
  taotoken: "TaoToken",
  opencode: "OpenCode Zen (OpenAI-compatible)",
};

/**
 * 把历史 provider 值归一化成 AtomCode 认的 id（`claude` → `anthropic` 等）。
 * 未知值原样返回：AtomCode 对未知 id 回退 openai-compatible，不阻断保存。
 */
export function normalizeAtomCodeProvider(value: string): string {
  switch (value) {
    case "claude":
      return "anthropic";
    case "openai_responses":
      return "openai-responses";
    case "gemini":
      return "openai-compatible";
    default:
      return value;
  }
}

export const atomCodeCustomTemplate = () =>
  ({
    account: { provider: "openai-compatible", base_url: "", api_key: "" },
    models: {},
  }) satisfies AtomCodeProviderPreset["settingsConfig"];

// 2026-10-25 起官方只留 Flash（Pro 贵且效果不如 Flash），与 Codex/Claude 预设一致。
// DeepSeek 用 AtomCode 自己的 `deepseek` 预设定（wire 仍是 openai chat，默认端点
// /v1）；原生 Messages 方言是另一个预设 `deepseek-messages`，需要时在下拉里选。
// context_window 不写：AtomCode 自带 DeepSeek 模型的窗口知识，留空即用它的默认。
// Anthropic 走 `anthropic` 预设（wire anthropic）；Ollama 走 `ollama`，
// base_url 留 localhost 默认端口。
// 其余厂商预设一律写 AtomCode 自己的厂商预设 id（与 TUI /provider 添加厂商时写出的
// 值相同），同时保留显式 base_url：老版本 AtomCode 不认识某个 id 时会回退
// openai-compatible，有 base_url 仍能继续工作。
// 2026-11-11 对齐 AtomCode 源码 provider_preset.rs（main，registry 19 个厂商）：
// 智谱 Coding Plan / Moonshot / MiniMax×2 / 小米 MiMo / LongCat / 火山方舟×2 /
// OpenRouter / TaoToken / OpenCode Zen；各家默认模型与窗口照抄本仓 Claude/Codex
// 预设已核实的数据。AtomGit（llm-api.atomgit.com/v1）依赖 AtomCode 自家登录态发
// 凭证，cc-switch 侧拿不到 API Key，故不内置。
export const atomcodeProviderPresets: AtomCodeProviderPreset[] = [
  {
    name: "Anthropic",
    providerKey: "anthropic",
    websiteUrl: "https://www.anthropic.com",
    apiKeyUrl: "https://console.anthropic.com/settings/keys",
    category: "cn_official",
    icon: "anthropic",
    iconColor: "#D4915D",
    settingsConfig: {
      account: {
        provider: "anthropic",
        base_url: "https://api.anthropic.com",
        api_key: "",
      },
      models: {
        "claude-sonnet-4-6": {
          model: "claude-sonnet-4-6",
          context_window: 128000,
        },
      },
    },
  },
  {
    name: "OpenAI",
    providerKey: "openai",
    websiteUrl: "https://platform.openai.com",
    apiKeyUrl: "https://platform.openai.com/api-keys",
    category: "cn_official",
    icon: "openai",
    iconColor: "#00A67E",
    settingsConfig: {
      account: {
        provider: "openai",
        base_url: "https://api.openai.com/v1",
        api_key: "",
      },
      models: {
        "gpt-5.2": { model: "gpt-5.2", context_window: 400000 },
      },
    },
  },
  {
    name: "DeepSeek",
    providerKey: "deepseek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    category: "cn_official",
    settingsConfig: {
      account: {
        provider: "deepseek",
        base_url: "https://api.deepseek.com/v1",
        api_key: "",
      },
      models: {
        "deepseek-flash": { model: "deepseek-flash" },
      },
    },
    icon: "deepseek",
    iconColor: "#1E88E5",
  },
  {
    name: "智谱 GLM",
    nameKey: "providerForm.presets.zhipu",
    family: "zhipu",
    planKey: "payg",
    providerKey: "zhipu",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://open.bigmodel.cn/usercenter/apikeys",
    category: "cn_official",
    settingsConfig: {
      account: {
        provider: "zhipu",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        api_key: "",
      },
      models: {
        "glm-4-plus": { model: "glm-4-plus", context_window: 128000 },
      },
    },
    icon: "zhipu",
  },
  {
    // GLM Coding Plan 的 OpenAI Chat 专用端点。智谱三端点分立（docs.bigmodel.cn/cn/
    // coding-plan/tool/others）：Anthropic /api/anthropic、OpenAI Chat
    // /api/coding/paas/v4、OpenAI Responses /api/v1，配错端点不消耗套餐额度。
    name: "智谱 GLM Coding Plan",
    nameKey: "providerForm.presets.zhipuCoding",
    family: "zhipu",
    planKey: "coding",
    providerKey: "zhipu-coding",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code",
    category: "cn_official",
    icon: "zhipu",
    settingsConfig: {
      account: {
        provider: "zhipu-coding",
        base_url: "https://open.bigmodel.cn/api/coding/paas/v4",
        api_key: "",
      },
      models: {
        "glm-5.3": { model: "glm-5.3", context_window: 1048576 },
      },
    },
  },
  {
    name: "通义千问 Qwen",
    nameKey: "providerForm.presets.qwen",
    providerKey: "aliyun",
    websiteUrl: "https://dashscope.aliyun.com",
    apiKeyUrl: "https://bailian.console.aliyun.com/?apiKey=1",
    category: "cn_official",
    settingsConfig: {
      account: {
        provider: "aliyun",
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        api_key: "",
      },
      models: {
        "qwen-plus": { model: "qwen-plus", context_window: 128000 },
      },
    },
    icon: "qwen",
  },
  {
    // 2026-11-03 盘点（同 Claude 预设）：K2 系列 2026-10-31 起对新用户停售、
    // 2026-12 月底退役，默认 K 系旗舰 kimi-k3.1-pro（1M 上下文）。
    name: "Kimi",
    providerKey: "moonshot",
    websiteUrl: "https://platform.kimi.com",
    apiKeyUrl: "https://platform.kimi.com/console/api-keys",
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
    settingsConfig: {
      account: {
        provider: "moonshot",
        base_url: "https://api.moonshot.cn/v1",
        api_key: "",
      },
      models: {
        "kimi-k3.1-pro": { model: "kimi-k3.1-pro", context_window: 1048576 },
      },
    },
  },
  {
    // MiniMax 域名拆分：国内按量走 api.minimaxi.com（platform.minimaxi.com 已 301
    // 回 platform.minimax.cn），Token Plan 订阅仍走 api.minimax.cn，密钥不通用。
    name: "MiniMax",
    family: "minimax",
    planKey: "payg",
    providerKey: "minimax",
    websiteUrl: "https://platform.minimax.cn",
    apiKeyUrl: "https://platform.minimax.cn/console/api-keys",
    category: "cn_official",
    icon: "minimax",
    iconColor: "#FF6B6B",
    settingsConfig: {
      account: {
        provider: "minimax",
        base_url: "https://api.minimaxi.com/v1",
        api_key: "",
      },
      models: {
        "MiniMax-M3.1-Flash": {
          model: "MiniMax-M3.1-Flash",
          context_window: 1000000,
        },
      },
    },
  },
  {
    name: "MiniMax Token Plan",
    family: "minimax",
    planKey: "tokenPlan",
    providerKey: "minimax-token-plan",
    websiteUrl: "https://platform.minimax.cn",
    apiKeyUrl: "https://platform.minimax.cn/subscribe/token-plan",
    category: "cn_official",
    icon: "minimax",
    iconColor: "#FF6B6B",
    settingsConfig: {
      account: {
        provider: "minimax-token-plan",
        base_url: "https://api.minimax.cn/v1",
        api_key: "",
      },
      models: {
        "MiniMax-M3.1-Flash": {
          model: "MiniMax-M3.1-Flash",
          context_window: 1000000,
        },
      },
    },
  },
  {
    name: "Xiaomi MiMo",
    providerKey: "xiaomi-mimo",
    websiteUrl: "https://platform.xiaomimimo.com",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/api-keys",
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
    settingsConfig: {
      account: {
        provider: "xiaomi-mimo",
        base_url: "https://api.xiaomimimo.com/v1",
        api_key: "",
      },
      models: {
        "mimo-v2.6-pro": { model: "mimo-v2.6-pro", context_window: 1048576 },
      },
    },
  },
  {
    // LongCat 的 OpenAI 兼容入口在 /openai/v1 子路径（与 Codex 预设一致）。
    name: "LongCat",
    providerKey: "longcat",
    websiteUrl: "https://longcat.chat/platform",
    apiKeyUrl: "https://longcat.chat/platform/api_keys",
    category: "cn_official",
    icon: "longcat",
    iconColor: "#29E154",
    settingsConfig: {
      account: {
        provider: "longcat",
        base_url: "https://api.longcat.chat/openai/v1",
        api_key: "",
      },
      models: {
        "LongCat-2.0": { model: "LongCat-2.0", context_window: 1048576 },
      },
    },
  },
  {
    name: "Volcengine Ark",
    family: "volcengine",
    planKey: "payg",
    providerKey: "volcengine",
    websiteUrl:
      "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey?apikey=%7B%7D&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey?apikey=%7B%7D&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    category: "cn_official",
    icon: "doubao",
    iconColor: "#3370FF",
    settingsConfig: {
      account: {
        provider: "volcengine",
        base_url: "https://ark.cn-beijing.volces.com/api/v3",
        api_key: "",
      },
      models: {
        "doubao-seed-2-1-pro-260915": {
          model: "doubao-seed-2-1-pro-260915",
          context_window: 262144,
        },
      },
    },
  },
  {
    // ⚠️ 计费红线（官方 warning）：Coding Plan 必须走 /api/coding/v3；按量端点
    // /api/v3 不消耗套餐额度、按量另计费——两者绝不能混用。
    name: "Volcengine Ark Coding Plan",
    family: "volcengine",
    planKey: "coding",
    providerKey: "volcengine-coding",
    websiteUrl:
      "https://www.volcengine.com/activity/codingplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://www.volcengine.com/activity/codingplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    category: "cn_official",
    icon: "huoshan",
    iconColor: "#3370FF",
    settingsConfig: {
      account: {
        provider: "volcengine-coding",
        base_url: "https://ark.cn-beijing.volces.com/api/coding/v3",
        api_key: "",
      },
      models: {
        "ark-code-latest": { model: "ark-code-latest", context_window: 256000 },
      },
    },
  },
  {
    name: "SiliconFlow",
    providerKey: "siliconflow",
    websiteUrl: "https://cloud.siliconflow.cn",
    apiKeyUrl: "https://cloud.siliconflow.cn/account/ak",
    category: "aggregator",
    settingsConfig: {
      account: {
        provider: "siliconflow",
        base_url: "https://api.siliconflow.cn/v1",
        api_key: "",
      },
      models: {
        "Qwen/Qwen2.5-72B-Instruct": { model: "Qwen/Qwen2.5-72B-Instruct" },
      },
    },
  },
  {
    // 模型键沿用表单 handleApplyModels 的清洗规则（非 [a-zA-Z0-9._-] 转 `-`），
    // model 字段保留 OpenRouter 的 vendor/model 原名。
    name: "OpenRouter",
    providerKey: "openrouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6566F1",
    settingsConfig: {
      account: {
        provider: "openrouter",
        base_url: "https://openrouter.ai/api/v1",
        api_key: "",
      },
      models: {
        "openai-gpt-6.1-sol": { model: "openai/gpt-6.1-sol" },
      },
    },
  },
  {
    // TaoToken 聚合中转：base_url 取 AtomCode 预设同款入口（taotoken.net/api/v1，
    // 官网另有等价的 api.taotoken.net/v1）。默认模型取官网热门榜的 GLM-5.3-Flash
    // （¥0.8/M，Coding Plan 亦支持）；全量 Model ID 见 https://taotoken.net/models。
    name: "TaoToken",
    providerKey: "taotoken",
    websiteUrl: "https://taotoken.net",
    apiKeyUrl: "https://taotoken.net/console/api-keys",
    category: "aggregator",
    settingsConfig: {
      account: {
        provider: "taotoken",
        base_url: "https://taotoken.net/api/v1",
        api_key: "",
      },
      models: {
        "GLM-5.3-Flash": { model: "GLM-5.3-Flash" },
      },
    },
  },
  {
    // OpenCode Zen 按量网关的 OpenAI 兼容入口；默认 gpt-6-sol（gpt-5.6-sol 在
    // Zen 上 403 Model access is disabled，见 Codex 预设 2026-10-06 真 Key 实测）。
    name: "OpenCode Zen",
    providerKey: "opencode",
    websiteUrl: "https://opencode.ai/zen",
    apiKeyUrl: "https://opencode.ai/auth",
    category: "aggregator",
    icon: "opencode",
    iconColor: "#211E1E",
    settingsConfig: {
      account: {
        provider: "opencode",
        base_url: "https://opencode.ai/zen/v1",
        api_key: "",
      },
      models: {
        "gpt-6-sol": { model: "gpt-6-sol" },
      },
    },
  },
  {
    // 本地 Ollama：先 `ollama pull llama3.2` 并启动服务，再切换到这个供应商。
    // function calling 兼容性参差不齐，弱一些的本地模型可能不稳定触发工具调用；
    // 优先尝试 Qwen2.5 / Llama3.2 的 Instruct 版本。
    name: "Ollama (本地)",
    nameKey: "providerForm.presets.ollamaLocal",
    providerKey: "ollama",
    websiteUrl: "https://ollama.com",
    category: "local",
    icon: "ollama",
    settingsConfig: {
      account: {
        provider: "ollama",
        base_url: "http://localhost:11434",
      },
      models: {
        "llama3.2": { model: "llama3.2", context_window: 8000 },
      },
    },
  },
];
