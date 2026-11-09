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
  | "deepseek"
  | "gemini"
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
  { value: "anthropic", label: "Anthropic Messages" },
  { value: "deepseek", label: "DeepSeek" },
  { value: "gemini", label: "Gemini" },
];

export const atomCodeCustomTemplate = () =>
  ({
    account: { provider: "openai", base_url: "", api_key: "" },
    models: {},
  }) satisfies AtomCodeProviderPreset["settingsConfig"];

// 2026-10-25 起官方只留 Flash（Pro 贵且效果不如 Flash），与 Codex/Claude 预设一致；
// `deepseek` 账号类型走 DeepSeek 原生方言，端点与 Codex 预设同为裸域名。
// context_window 不写：AtomCode 自带 DeepSeek 模型的窗口知识，留空即用它的默认。
export const atomcodeProviderPresets: AtomCodeProviderPreset[] = [
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
];
