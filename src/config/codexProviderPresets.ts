/**
 * Codex 预设供应商配置模板
 */
import { ProviderCategory } from "../types";

export interface CodexProviderPreset {
  name: string;
  websiteUrl: string;
  auth: Record<string, any>; // 将写入 ~/.codex/auth.json
  config: string; // 将写入 ~/.codex/config.toml（TOML 字符串）
  isOfficial?: boolean; // 标识是否为官方预设
  category?: ProviderCategory; // 新增：分类
  // 二级选项配置
  endpoints?: string[];
  enableAutoSpeed?: boolean; // 是否启用自动测速
}

export const codexProviderPresets: CodexProviderPreset[] = [
  {
    name: "Codex官方",
    websiteUrl: "https://chatgpt.com/codex",
    isOfficial: true,
    category: "official",
    // 官方的 key 为null
    auth: {
      OPENAI_API_KEY: null,
    },
    config: ``,
  },
  {
    name: "PackyCode",
    websiteUrl: "https://codex.packycode.com/",
    category: "third_party",
    // PackyCode 一般通过 API Key；请将占位符替换为你的实际 key
    auth: {
      OPENAI_API_KEY: "sk-your-api-key-here",
    },
    config: `model_provider = "packycode"
model = "gpt-5"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.packycode]
name = "packycode"
base_url = "https://codex-api.packycode.com/v1"
wire_api = "responses"
env_key = "packycode"`,
    endpoints: [
      "https://codex-api.packycode.com",
      "https://codex-api-hk-cn2.packycode.com",
    ],
    enableAutoSpeed: true,
  },
];
