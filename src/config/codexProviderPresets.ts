/**
 * Codex 预设供应商配置模板
 */
import { ProviderCategory } from "../types";

export interface CodexProviderPreset {
  name: string;
  websiteUrl: string;
  // 第三方供应商可提供单独的获取 API Key 链接
  apiKeyUrl?: string;
  auth: Record<string, any>; // 将写入 ~/.codex/auth.json
  config: string; // 将写入 ~/.codex/config.toml（TOML 字符串）
  isOfficial?: boolean; // 标识是否为官方预设
  category?: ProviderCategory; // 新增：分类
  isCustomTemplate?: boolean; // 标识是否为自定义模板
}

/**
 * 生成第三方供应商的 auth.json
 */
export function generateThirdPartyAuth(apiKey: string): Record<string, any> {
  return {
    OPENAI_API_KEY: apiKey || "sk-your-api-key-here",
  };
}

/**
 * 生成第三方供应商的 config.toml
 */
export function generateThirdPartyConfig(
  providerName: string,
  baseUrl: string,
  modelName = "gpt-5-codex"
): string {
  // 清理供应商名称，确保符合TOML键名规范
  const cleanProviderName =
    providerName
      .toLowerCase()
      .replace(/[^a-z0-9_]/g, "_")
      .replace(/^_+|_+$/g, "") || "custom";

  return `model_provider = "${cleanProviderName}"
model = "${modelName}"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.${cleanProviderName}]
name = "${cleanProviderName}"
base_url = "${baseUrl}"
wire_api = "responses"`;
}

export const codexProviderPresets: CodexProviderPreset[] = [
  {
    name: "Codex官方",
    websiteUrl: "https://chatgpt.com/codex",
    isOfficial: true,
    category: "official",
    auth: {
      OPENAI_API_KEY: null,
    },
    config: ``,
  },
  {
    name: "PackyCode",
    websiteUrl: "https://codex.packycode.com/",
    category: "third_party",
    auth: generateThirdPartyAuth("sk-your-api-key-here"),
    config: generateThirdPartyConfig(
      "packycode",
      "https://codex-api.packycode.com/v1",
      "gpt-5-codex"
    ),
  },
];
