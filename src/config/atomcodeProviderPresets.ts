import type { ProviderCategory } from "@/types";

/**
 * AtomCode（共存型）供应商预设：settingsConfig 直接对应后端
 * `atomcode_config::validate_provider` 校验的形状 —— `account` 一个账号节点
 * 加 `models` 下的模型表；写入 config.toml 时后端会把 `provider_accounts.<id>`
 * 设为 account、把每个模型的 `account` 字段改成账号键，所以这里不用写它。
 */
export type AtomCodeProviderType =
  | "openai"
  | "anthropic"
  | "claude"
  | "deepseek"
  | "gemini"
  | "ollama"
  | "openai_responses";

export interface AtomCodeModelEntry {
  model: string;
  context_window?: number;
}

export interface AtomCodeProviderPreset {
  name: string;
  /** config.toml 里的账号键：字母、数字、`-`、`_`、`.`（后端 validate_provider 同款） */
  providerKey: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  category: ProviderCategory;
  icon?: string;
  iconColor?: string;
  settingsConfig: {
    account: {
      provider: AtomCodeProviderType;
      base_url: string;
      api_key?: string;
    };
    models: Record<string, AtomCodeModelEntry>;
  };
}

export const ATOMCODE_PROVIDER_TYPES: {
  value: AtomCodeProviderType;
  label: string;
}[] = [
  { value: "openai", label: "OpenAI Chat Completions" },
  { value: "openai_responses", label: "OpenAI Responses" },
  { value: "claude", label: "Claude (Anthropic Messages)" },
  // 旧版集成把 Anthropic 存成 `anthropic`；保留选项以便历史供应商仍可正常编辑保存
  { value: "anthropic", label: "Anthropic Messages (alias)" },
  { value: "deepseek", label: "DeepSeek" },
  { value: "gemini", label: "Gemini" },
  { value: "ollama", label: "Ollama (local)" },
];

export const atomCodeCustomTemplate = () =>
  ({
    account: { provider: "openai", base_url: "", api_key: "" },
    models: {},
  }) satisfies AtomCodeProviderPreset["settingsConfig"];

// 2026-10-25 起官方只留 Flash（Pro 贵且效果不如 Flash），与 Codex/Claude 预设一致；
// `deepseek` 账号类型走 DeepSeek 原生方言，端点与 Codex 预设同为裸域名。
// context_window 不写：AtomCode 自带 DeepSeek 模型的窗口知识，留空即用它的默认。
// Claude 走 AtomCode 文档中的 `claude` 类型（`anthropic` 作为历史别名仍被后端接受）；
// Ollama 走原生 `ollama` 类型，base_url 留 localhost 默认端口。
// 其余预设均为 OpenAI 兼容协议（provider: "openai"）。
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
        provider: "claude",
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
        base_url: "https://api.deepseek.com",
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
    providerKey: "glm",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://open.bigmodel.cn/usercenter/apikeys",
    category: "cn_official",
    settingsConfig: {
      account: {
        provider: "openai",
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
    name: "通义千问 Qwen",
    providerKey: "qwen",
    websiteUrl: "https://dashscope.aliyun.com",
    apiKeyUrl: "https://bailian.console.aliyun.com/?apiKey=1",
    category: "cn_official",
    settingsConfig: {
      account: {
        provider: "openai",
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
    name: "SiliconFlow",
    providerKey: "siliconflow",
    websiteUrl: "https://cloud.siliconflow.cn",
    apiKeyUrl: "https://cloud.siliconflow.cn/account/ak",
    category: "aggregator",
    settingsConfig: {
      account: {
        provider: "openai",
        base_url: "https://api.siliconflow.cn/v1",
        api_key: "",
      },
      models: {
        "Qwen/Qwen2.5-72B-Instruct": { model: "Qwen/Qwen2.5-72B-Instruct" },
      },
    },
  },
  {
    // 本地 Ollama：先 `ollama pull llama3.2` 并启动服务，再切换到这个供应商。
    // function calling 兼容性参差不齐，弱一些的本地模型可能不稳定触发工具调用；
    // 优先尝试 Qwen2.5 / Llama3.2 的 Instruct 版本。
    name: "Ollama (本地)",
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
